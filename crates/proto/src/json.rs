//! Hand-rolled JSON, in the house style: stdlib only, decode never panics.
//! The surface is exactly what its two consumers need — the mcp server
//! parses a request line into a tree and writes a response tree on one
//! line; the talent codec (`crate::talents`) reads the generated dataset.
//! Object order is preserved (a `Vec` of pairs, not a map) so output is
//! deterministic and golden-testable.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    pub fn num(n: impl Into<f64>) -> Json {
        Json::Num(n.into())
    }

    /// u64 → Num. Above 2^53 precision is gone, but every quantity here is a
    /// fight statistic, orders of magnitude below that cliff.
    pub fn u64(n: u64) -> Json {
        Json::Num(n as f64)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= u64::MAX as f64 => {
                Some(*n as u64)
            }
            _ => None,
        }
    }

    /// Serialize onto one line (no interior newlines — the framing).
    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if n.is_finite() {
                    // Integers print without the ".0" noise LLMs and jq alike
                    // would have to see past.
                    if n.fract() == 0.0 && n.abs() < 1e15 {
                        let _ = write!(out, "{}", *n as i64);
                    } else {
                        let _ = write!(out, "{n}");
                    }
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => write_escaped(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    pub fn to_line(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
}

fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Nesting cap: an MCP request is a few levels deep; a thousand-bracket
/// bomb is not a request.
const MAX_DEPTH: u32 = 64;

pub fn parse(input: &str) -> Result<Json, String> {
    let mut p = Parser {
        chars: input.chars().peekable(),
    };
    p.skip_ws();
    let v = p.value(0)?;
    p.skip_ws();
    match p.chars.next() {
        None => Ok(v),
        Some(c) => Err(format!("trailing {c:?} after the value")),
    }
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.chars.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.chars.next();
        }
    }

    fn expect(&mut self, want: char) -> Result<(), String> {
        match self.chars.next() {
            Some(c) if c == want => Ok(()),
            Some(c) => Err(format!("expected {want:?}, found {c:?}")),
            None => Err(format!("expected {want:?}, found end of input")),
        }
    }

    fn value(&mut self, depth: u32) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nesting too deep".to_string());
        }
        self.skip_ws();
        match self.chars.peek().copied() {
            Some('{') => self.object(depth),
            Some('[') => self.array(depth),
            Some('"') => self.string().map(Json::Str),
            Some('t') => self.literal("true", Json::Bool(true)),
            Some('f') => self.literal("false", Json::Bool(false)),
            Some('n') => self.literal("null", Json::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected {c:?}")),
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
        for want in word.chars() {
            match self.chars.next() {
                Some(c) if c == want => {}
                _ => return Err(format!("bad literal, expected {word:?}")),
            }
        }
        Ok(v)
    }

    fn number(&mut self) -> Result<Json, String> {
        let mut tok = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E') {
                tok.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        tok.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Json::Num)
            .ok_or_else(|| format!("bad number {tok:?}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.chars.next() {
                None => return Err("unterminated string".to_string()),
                Some('"') => return Ok(out),
                Some('\\') => match self.chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('b') => out.push('\u{8}'),
                    Some('f') => out.push('\u{c}'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('u') => out.push(self.unicode_escape()?),
                    other => return Err(format!("bad escape {other:?}")),
                },
                Some(c) => out.push(c),
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let hex4 = |p: &mut Self| -> Result<u32, String> {
            let mut n = 0u32;
            for _ in 0..4 {
                let c = p.chars.next().ok_or("truncated \\u escape")?;
                let d = c.to_digit(16).ok_or_else(|| format!("bad hex {c:?}"))?;
                n = n * 16 + d;
            }
            Ok(n)
        };
        let hi = hex4(self)?;
        // Surrogate pair: a second \uXXXX must follow.
        if (0xD800..0xDC00).contains(&hi) {
            if self.chars.next() != Some('\\') || self.chars.next() != Some('u') {
                return Err("lone high surrogate".to_string());
            }
            let lo = hex4(self)?;
            if !(0xDC00..0xE000).contains(&lo) {
                return Err("bad low surrogate".to_string());
            }
            let n = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
            return char::from_u32(n).ok_or_else(|| "bad surrogate pair".to_string());
        }
        char::from_u32(hi).ok_or_else(|| format!("bad codepoint \\u{hi:04x}"))
    }

    fn object(&mut self, depth: u32) -> Result<Json, String> {
        self.expect('{')?;
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.chars.peek() == Some(&'}') {
            self.chars.next();
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(':')?;
            let val = self.value(depth + 1)?;
            pairs.push((key, val));
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some('}') => return Ok(Json::Obj(pairs)),
                other => return Err(format!("expected , or }} in object, found {other:?}")),
            }
        }
    }

    fn array(&mut self, depth: u32) -> Result<Json, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.chars.peek() == Some(&']') {
            self.chars.next();
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some(']') => return Ok(Json::Arr(items)),
                other => return Err(format!("expected , or ] in array, found {other:?}")),
            }
        }
    }
}

/// Shorthand for building object trees without the tuple noise.
#[macro_export]
macro_rules! obj {
    ($($key:literal : $val:expr),* $(,)?) => {
        $crate::json::Json::Obj(vec![$(($key.to_string(), $val)),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(src: &str) -> String {
        parse(src).unwrap().to_line()
    }

    #[test]
    fn values_roundtrip() {
        assert_eq!(roundtrip("null"), "null");
        assert_eq!(roundtrip("true"), "true");
        assert_eq!(roundtrip("42"), "42");
        assert_eq!(roundtrip("-1.5"), "-1.5");
        assert_eq!(roundtrip(r#""a\"b\\c\nd""#), r#""a\"b\\c\nd""#);
        assert_eq!(roundtrip(r#"[1,[2,"x"],{}]"#), r#"[1,[2,"x"],{}]"#);
        assert_eq!(
            roundtrip(r#"{ "a" : 1 , "b" : [ true , null ] }"#),
            r#"{"a":1,"b":[true,null]}"#
        );
    }

    #[test]
    fn object_order_and_lookup_survive() {
        let v = parse(r#"{"z":1,"a":{"nested":"yes"}}"#).unwrap();
        assert_eq!(v.to_line(), r#"{"z":1,"a":{"nested":"yes"}}"#);
        assert_eq!(
            v.get("a")
                .and_then(|a| a.get("nested"))
                .and_then(Json::as_str),
            Some("yes")
        );
        assert_eq!(v.get("z").and_then(Json::as_u64), Some(1));
        assert_eq!(v.get("missing"), None);
    }

    #[test]
    fn unicode_escapes_decode() {
        assert_eq!(roundtrip(r#""\u00e9\u2211""#), "\"\u{e9}\u{2211}\"");
        // Surrogate pair: 😀
        assert_eq!(parse(r#""\ud83d\ude00""#).unwrap(), Json::Str("😀".into()));
        assert!(parse(r#""\ud83d""#).is_err());
    }

    #[test]
    fn control_chars_escape_on_output() {
        assert_eq!(Json::str("a\u{1}b").to_line(), r#""a\u0001b""#);
    }

    #[test]
    fn garbage_is_an_error_never_a_panic() {
        for src in [
            "",
            "{",
            "}",
            "[",
            "]",
            "{\"a\"}",
            "{\"a\":}",
            "[1,]",
            "{,}",
            "tru",
            "nul",
            "01x",
            "\"",
            "\"\\q\"",
            "\"\\u12\"",
            "1 2",
            "{\"a\":1,}",
            "--1",
            "1e999",
            "NaN",
            "\u{7f}",
            "[\"\\ud800x\"]",
        ] {
            assert!(parse(src).is_err(), "{src:?} must not parse");
        }
        // A bracket bomb hits the depth cap instead of the stack.
        let bomb = "[".repeat(100_000);
        assert!(parse(&bomb).is_err());
    }

    #[test]
    fn the_obj_macro_builds_ordered_objects() {
        let v = obj! { "b": Json::u64(2), "a": Json::str("x") };
        assert_eq!(v.to_line(), r#"{"b":2,"a":"x"}"#);
    }

    #[test]
    fn nonfinite_numbers_serialize_as_null() {
        assert_eq!(Json::Num(f64::NAN).to_line(), "null");
        assert_eq!(Json::Num(f64::INFINITY).to_line(), "null");
    }
}

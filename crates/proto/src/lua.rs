//! A reader for the Lua the game writes to `SavedVariables/*.lua`: a
//! sequence of `NAME = <value>` assignments where a value is `nil`, a
//! boolean, a number, a string or a table of `[key] = value` entries.
//! Hand-rolled like `json` so every reader stays stdlib-only; it is a
//! reader of the game's serializer output, not a Lua interpreter — no
//! expressions, no functions — and the addon's own file is the only one
//! anything here opens.

use std::fmt;

/// A Lua value as the serializer writes it. Table entries keep file order;
/// a positional entry (`{ "a", "b" }`) gets its 1-based index as its key,
/// exactly as Lua would.
#[derive(Debug, Clone, PartialEq)]
pub enum Lua {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Table(Vec<(Key, Lua)>),
}

/// A table key: an integer, or a string (a non-integer numeric key — never
/// written by the game — is kept as its decimal text).
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Int(i64),
    Str(String),
}

impl Lua {
    /// A string-keyed entry of a table.
    pub fn get(&self, key: &str) -> Option<&Lua> {
        match self {
            Lua::Table(entries) => entries
                .iter()
                .find(|(k, _)| matches!(k, Key::Str(s) if s == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Lua::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Lua::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Lua::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&[(Key, Lua)]> {
        match self {
            Lua::Table(entries) => Some(entries),
            _ => None,
        }
    }
}

impl Key {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Key::Str(s) => Some(s),
            Key::Int(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

/// Every top-level `NAME = value` in `text`, in order. A byte the grammar
/// cannot place is an error naming its line; a truncated file (the game was
/// killed mid-write) is an error rather than a partial table.
pub fn parse(text: &str) -> Result<Vec<(String, Lua)>, Error> {
    let mut p = Parser {
        src: text.as_bytes(),
        pos: 0,
        line: 1,
    };
    let mut out = Vec::new();
    loop {
        p.skip_ws();
        if p.pos >= p.src.len() {
            return Ok(out);
        }
        let name = p.ident().ok_or_else(|| p.err("expected a global name"))?;
        p.skip_ws();
        if !p.eat(b'=') {
            return Err(p.err("expected `=` after the global name"));
        }
        p.skip_ws();
        let value = p.value(0)?;
        out.push((name, value));
    }
}

/// Table nesting the reader will follow; the game's files go a dozen deep
/// at most, and a hostile file must not blow the stack.
const MAX_DEPTH: usize = 64;

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
}

impl Parser<'_> {
    fn err(&self, msg: &str) -> Error {
        Error {
            line: self.line,
            msg: msg.to_string(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Whitespace and comments (`-- …` to end of line, `--[[ … ]]` blocks).
    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => {
                    self.bump();
                }
                Some(b'-') if self.src.get(self.pos + 1) == Some(&b'-') => {
                    self.pos += 2;
                    if self.src.get(self.pos..self.pos + 2) == Some(b"[[") {
                        self.pos += 2;
                        while self.pos < self.src.len()
                            && self.src.get(self.pos..self.pos + 2) != Some(b"]]")
                        {
                            self.bump();
                        }
                        self.pos = (self.pos + 2).min(self.src.len());
                    } else {
                        while let Some(c) = self.peek() {
                            if c == b'\n' {
                                break;
                            }
                            self.pos += 1;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn ident(&mut self) -> Option<String> {
        let start = self.pos;
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {}
            _ => return None,
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(self.src.get(start..self.pos)?)
            .ok()
            .map(str::to_string)
    }

    fn value(&mut self, depth: usize) -> Result<Lua, Error> {
        match self.peek() {
            None => Err(self.err("unexpected end of file")),
            Some(b'{') => self.table(depth),
            Some(b'"' | b'\'') => self.string().map(Lua::Str),
            Some(b'[') if self.src.get(self.pos + 1) == Some(&b'[') => {
                self.long_string().map(Lua::Str)
            }
            Some(c) if c.is_ascii_digit() || c == b'-' || c == b'.' => self.number(),
            Some(_) => {
                let word = self.ident().ok_or_else(|| self.err("expected a value"))?;
                match word.as_str() {
                    "nil" => Ok(Lua::Nil),
                    "true" => Ok(Lua::Bool(true)),
                    "false" => Ok(Lua::Bool(false)),
                    // How `tostring` spells the non-finite floats.
                    "inf" => Ok(Lua::Num(f64::INFINITY)),
                    "nan" => Ok(Lua::Num(f64::NAN)),
                    _ => Err(self.err(&format!("unexpected word {word:?}"))),
                }
            }
        }
    }

    fn table(&mut self, depth: usize) -> Result<Lua, Error> {
        if depth >= MAX_DEPTH {
            return Err(self.err("tables nested too deep"));
        }
        self.bump(); // {
        let mut entries: Vec<(Key, Lua)> = Vec::new();
        let mut next_index: i64 = 1;
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err(self.err("unterminated table")),
                Some(b'}') => {
                    self.bump();
                    return Ok(Lua::Table(entries));
                }
                Some(b',' | b';') => {
                    self.bump();
                    continue;
                }
                Some(b'[') if self.src.get(self.pos + 1) != Some(&b'[') => {
                    self.bump();
                    self.skip_ws();
                    let key = match self.value(depth + 1)? {
                        Lua::Str(s) => Key::Str(s),
                        Lua::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Key::Int(n as i64),
                        Lua::Num(n) => Key::Str(n.to_string()),
                        Lua::Bool(b) => Key::Str(b.to_string()),
                        _ => return Err(self.err("a table key must be a string or a number")),
                    };
                    self.skip_ws();
                    if !self.eat(b']') {
                        return Err(self.err("expected `]` after the key"));
                    }
                    self.skip_ws();
                    if !self.eat(b'=') {
                        return Err(self.err("expected `=` after the key"));
                    }
                    self.skip_ws();
                    let value = self.value(depth + 1)?;
                    entries.push((key, value));
                }
                Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                    // `name = value`, or a bare word value (`true`, `nil`).
                    let save = (self.pos, self.line);
                    let word = self.ident().unwrap_or_default();
                    self.skip_ws();
                    if self.eat(b'=') {
                        self.skip_ws();
                        let value = self.value(depth + 1)?;
                        entries.push((Key::Str(word), value));
                    } else {
                        (self.pos, self.line) = save;
                        let value = self.value(depth + 1)?;
                        entries.push((Key::Int(next_index), value));
                        next_index += 1;
                    }
                }
                Some(_) => {
                    let value = self.value(depth + 1)?;
                    entries.push((Key::Int(next_index), value));
                    next_index += 1;
                }
            }
        }
    }

    fn number(&mut self) -> Result<Lua, Error> {
        let negative = self.eat(b'-');
        self.skip_ws();
        if negative && self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            return match self.ident().as_deref() {
                Some("inf") => Ok(Lua::Num(f64::NEG_INFINITY)),
                Some("nan") => Ok(Lua::Num(f64::NAN)),
                _ => Err(self.err("expected a number after `-`")),
            };
        }
        if self.src.get(self.pos..self.pos + 2) == Some(b"0x")
            || self.src.get(self.pos..self.pos + 2) == Some(b"0X")
        {
            self.pos += 2;
            let hs = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let digits = std::str::from_utf8(self.src.get(hs..self.pos).unwrap_or_default())
                .unwrap_or_default();
            let n = u64::from_str_radix(digits, 16)
                .map_err(|_| self.err("bad hexadecimal number"))? as f64;
            return Ok(Lua::Num(if negative { -n } else { n }));
        }
        let ds = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            // A sign only continues an exponent.
            if matches!(self.peek(), Some(b'+' | b'-'))
                && !matches!(self.src.get(self.pos - 1), Some(b'e' | b'E'))
            {
                break;
            }
            self.pos += 1;
        }
        let text =
            std::str::from_utf8(self.src.get(ds..self.pos).unwrap_or_default()).unwrap_or_default();
        let mut n: f64 = text
            .parse()
            .map_err(|_| self.err(&format!("bad number {text:?}")))?;
        if negative {
            n = -n;
        }
        // The serializer writes `1/0`, `-1/0` and `0/0` for the floats it
        // cannot spell; nothing else in a saved file is an expression.
        let save = (self.pos, self.line);
        self.skip_ws();
        if self.eat(b'/') {
            self.skip_ws();
            let dstart = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
                self.pos += 1;
            }
            let d: f64 = std::str::from_utf8(self.src.get(dstart..self.pos).unwrap_or_default())
                .unwrap_or_default()
                .parse()
                .map_err(|_| self.err("bad division"))?;
            return Ok(Lua::Num(n / d));
        }
        (self.pos, self.line) = save;
        Ok(Lua::Num(n))
    }

    /// A quoted string with Lua's escapes: `\n \r \t \\ \" \' \a \b \f \v`,
    /// `\<newline>`, and decimal `\ddd` bytes (how the game spells control
    /// characters; multibyte UTF-8 is written raw).
    fn string(&mut self) -> Result<String, Error> {
        let quote = self.bump().unwrap_or(b'"');
        let mut bytes = Vec::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(self.err("unterminated string"));
            };
            match c {
                c if c == quote => break,
                b'\n' => return Err(self.err("newline inside a string")),
                b'\\' => {
                    let Some(e) = self.bump() else {
                        return Err(self.err("unterminated escape"));
                    };
                    match e {
                        b'n' => bytes.push(b'\n'),
                        b'r' => bytes.push(b'\r'),
                        b't' => bytes.push(b'\t'),
                        b'a' => bytes.push(7),
                        b'b' => bytes.push(8),
                        b'f' => bytes.push(12),
                        b'v' => bytes.push(11),
                        b'\\' | b'"' | b'\'' | b'\n' => bytes.push(e),
                        b'0'..=b'9' => {
                            let mut n = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d) if d.is_ascii_digit() => {
                                        self.pos += 1;
                                        n = n * 10 + u32::from(d - b'0');
                                    }
                                    _ => break,
                                }
                            }
                            let byte =
                                u8::try_from(n).map_err(|_| self.err("escape byte over 255"))?;
                            bytes.push(byte);
                        }
                        other => {
                            return Err(self.err(&format!("unknown escape \\{}", other as char)));
                        }
                    }
                }
                c => bytes.push(c),
            }
        }
        // The game writes UTF-8; a `\ddd` run may spell a multibyte
        // character byte by byte, which is why decoding waits for the end.
        String::from_utf8(bytes).map_err(|_| self.err("string is not UTF-8"))
    }

    /// `[[ … ]]` (no nesting levels: the serializer never writes them).
    fn long_string(&mut self) -> Result<String, Error> {
        self.pos += 2;
        let start = self.pos;
        while self.pos < self.src.len() {
            if self.src.get(self.pos..self.pos + 2) == Some(b"]]") {
                let s = std::str::from_utf8(self.src.get(start..self.pos).unwrap_or_default())
                    .map_err(|_| self.err("string is not UTF-8"))?
                    .to_string();
                self.pos += 2;
                return Ok(s);
            }
            self.bump();
        }
        Err(self.err("unterminated long string"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Lua {
        let mut all = parse(text).unwrap();
        assert_eq!(all.len(), 1, "{all:?}");
        all.pop().unwrap().1
    }

    #[test]
    fn scalars_and_the_serializers_number_spellings() {
        assert_eq!(one("X = nil"), Lua::Nil);
        assert_eq!(one("X = true"), Lua::Bool(true));
        assert_eq!(one("X = 42"), Lua::Num(42.0));
        assert_eq!(one("X = -7.5"), Lua::Num(-7.5));
        assert_eq!(one("X = 1e3"), Lua::Num(1000.0));
        assert_eq!(one("X = 0x1F"), Lua::Num(31.0));
        assert_eq!(one("X = 1/0"), Lua::Num(f64::INFINITY));
        assert_eq!(one("X = -1/0"), Lua::Num(f64::NEG_INFINITY));
        assert_eq!(one("X = -inf"), Lua::Num(f64::NEG_INFINITY));
        assert_eq!(one("X = inf"), Lua::Num(f64::INFINITY));
        assert!(one("X = 0/0").as_f64().unwrap().is_nan());
        assert_eq!(one("X = 7445431852"), Lua::Num(7_445_431_852.0));
    }

    #[test]
    fn strings_decode_every_escape_the_game_writes() {
        assert_eq!(one(r#"X = "a\"b\\c\n""#), Lua::Str("a\"b\\c\n".to_string()));
        assert_eq!(one("X = 'single'"), Lua::Str("single".to_string()));
        // Raw UTF-8 and the byte-by-byte spelling decode alike.
        assert_eq!(one("X = \"Ðark\""), Lua::Str("Ðark".to_string()));
        assert_eq!(one(r#"X = "\195\144ark""#), Lua::Str("Ðark".to_string()));
        assert_eq!(one(r#"X = "tab\9here""#), Lua::Str("tab\there".to_string()));
        assert_eq!(
            one("X = [[long\nform]]"),
            Lua::Str("long\nform".to_string())
        );
        assert!(parse("X = \"unterminated").is_err());
        assert!(parse(r#"X = "\300""#).is_err());
    }

    #[test]
    fn tables_keep_order_and_number_positional_entries() {
        let t = one(r#"
            X = {
                ["players"] = {
                    ["Player-1-A"] = { ["guild"] = "Templars", ["seen"] = 1700000000, },
                },
                [3] = "three",
                bare = false,
                "first", "second";
                nested = { { 1, 2 }, { [10] = true } },
            }
            "#);
        let players = t.get("players").unwrap();
        let rec = players.get("Player-1-A").unwrap();
        assert_eq!(rec.get("guild").and_then(Lua::as_str), Some("Templars"));
        assert_eq!(rec.get("seen").and_then(Lua::as_f64), Some(1.7e9));
        let entries = t.as_table().unwrap();
        assert_eq!(entries[1].0, Key::Int(3));
        assert_eq!(entries[2], (Key::Str("bare".into()), Lua::Bool(false)));
        assert_eq!(entries[3], (Key::Int(1), Lua::Str("first".into())));
        assert_eq!(entries[4], (Key::Int(2), Lua::Str("second".into())));
        let nested = t.get("nested").unwrap().as_table().unwrap();
        assert_eq!(nested[0].0, Key::Int(1));
        assert_eq!(
            nested[1].1.as_table().unwrap()[0],
            (Key::Int(10), Lua::Bool(true))
        );
    }

    #[test]
    fn several_globals_comments_and_crlf_read_like_the_games_files() {
        let text = "\r\n-- comment\r\nA_CONFIG = {\r\n\t[\"debug\"] = false,\r\n}\r\nB = 3 --[[ block\r\ncomment ]] C = \"x\"\r\n";
        let all = parse(text).unwrap();
        let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["A_CONFIG", "B", "C"]);
        assert_eq!(all[0].1.get("debug"), Some(&Lua::Bool(false)));
        assert_eq!(parse("").unwrap(), Vec::new());
        assert_eq!(parse("  \n-- only a comment\n").unwrap(), Vec::new());
    }

    #[test]
    fn errors_name_their_line_and_never_panic() {
        let e = parse("A = {\n  [\"k\"] = {\n").unwrap_err();
        assert_eq!(e.line, 3, "{e}");
        assert!(parse("A = ").is_err());
        assert!(parse("= 1").is_err());
        assert!(parse("A = {}}").is_err());
        assert!(parse("A = { [true] = }").is_err());
        assert!(parse("A = { [{}] = 1 }").is_err());
        assert!(parse("A = @").is_err());
        assert!(parse("A = 1 B").is_err(), "a global without a value");
        assert!(parse("A = -x").is_err());
        // Deep nesting is refused, not recursed into.
        let deep = format!("A = {}{}", "{".repeat(200), "}".repeat(200));
        assert!(parse(&deep).is_err());
        // Every prefix of a valid file is an error or a prefix answer.
        let full = "T = { [\"a\"] = \"x\\n\", [2] = 1/0, b = { true, nil } }";
        for (n, _) in full.char_indices() {
            let _ = parse(full.get(..n).unwrap_or_default());
        }
    }
}

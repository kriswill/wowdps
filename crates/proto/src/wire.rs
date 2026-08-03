//! Hand-rolled binary primitives: fixed-width little-endian integers,
//! length-prefixed strings and vecs, presence-byte options, and the
//! `u32 len | u8 tag | body` frame. Decoding returns `Result` — it never
//! panics, whatever the bytes say.

use std::io::{self, Read, Write};

/// Hard ceiling on one frame (`len` covers tag + body). Every message the
/// protocol can produce is bounded well under this; a frame that exceeds it
/// is a bug or garbage, not a condition to handle.
pub const MAX_FRAME: u32 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    UnexpectedEof,
    BadTag(u8),
    BadBool(u8),
    BadUtf8,
    FrameTooLarge,
    /// A message decoded cleanly but bytes were left over — same-version
    /// peers always agree on length, so leftovers mean corruption.
    TrailingBytes,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnexpectedEof => write!(f, "frame truncated"),
            DecodeError::BadTag(t) => write!(f, "unknown tag {t:#04x}"),
            DecodeError::BadBool(b) => write!(f, "bad bool byte {b:#04x}"),
            DecodeError::BadUtf8 => write!(f, "invalid utf-8 in string"),
            DecodeError::FrameTooLarge => write!(f, "frame exceeds {MAX_FRAME} bytes"),
            DecodeError::TrailingBytes => write!(f, "trailing bytes after message"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub type Result<T> = std::result::Result<T, DecodeError>;

// ---- encoding ---------------------------------------------------------------

pub fn put_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

pub fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn put_i64(buf: &mut Vec<u8>, v: i64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn put_f64(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_bits().to_le_bytes());
}

pub fn put_bool(buf: &mut Vec<u8>, v: bool) {
    buf.push(v as u8);
}

pub fn put_str(buf: &mut Vec<u8>, s: &str) {
    put_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

pub fn put_opt<T>(buf: &mut Vec<u8>, v: Option<&T>, f: impl FnOnce(&mut Vec<u8>, &T)) {
    match v {
        None => put_bool(buf, false),
        Some(t) => {
            put_bool(buf, true);
            f(buf, t);
        }
    }
}

pub fn put_vec<T>(buf: &mut Vec<u8>, items: &[T], mut f: impl FnMut(&mut Vec<u8>, &T)) {
    put_u32(buf, items.len() as u32);
    for item in items {
        f(buf, item);
    }
}

// ---- decoding ---------------------------------------------------------------

/// A cursor over one frame body. Every read is bounds-checked; a count or
/// length field the bytes lie about surfaces as `UnexpectedEof`, never as a
/// panic or an allocation sized by the attacker.
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.buf.len() < n {
            return Err(DecodeError::UnexpectedEof);
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }

    pub fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(DecodeError::BadBool(b)),
        }
    }

    pub fn string(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::BadUtf8)
    }

    pub fn opt<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<Option<T>> {
        if self.bool()? {
            Ok(Some(f(self)?))
        } else {
            Ok(None)
        }
    }

    pub fn vec<T>(&mut self, mut f: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let count = self.u32()?;
        // No `with_capacity(count)`: the count is untrusted; growth is paid
        // only as real items decode.
        let mut out = Vec::new();
        for _ in 0..count {
            out.push(f(self)?);
        }
        Ok(out)
    }

    /// Every message decode ends here: leftovers are corruption, not slack.
    pub fn finish(&self) -> Result<()> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }
}

// ---- framing ----------------------------------------------------------------

/// Build one on-the-wire frame: `u32 len | u8 tag | body`.
pub fn frame(tag: u8, body: &[u8]) -> Vec<u8> {
    let len = (body.len() + 1) as u32;
    debug_assert!(len <= MAX_FRAME, "outgoing frame over MAX_FRAME is a bug");
    let mut out = Vec::with_capacity(4 + len as usize);
    put_u32(&mut out, len);
    put_u8(&mut out, tag);
    out.extend_from_slice(body);
    out
}

/// Split one complete frame off the front of `bytes` (for in-process
/// transports and tests): returns `(tag, body, rest)`.
pub fn split_frame(bytes: &[u8]) -> Result<(u8, &[u8], &[u8])> {
    let mut rd = Reader::new(bytes);
    let len = rd.u32()?;
    if len > MAX_FRAME {
        return Err(DecodeError::FrameTooLarge);
    }
    if len == 0 {
        return Err(DecodeError::UnexpectedEof);
    }
    let rest_offset = 4 + len as usize;
    if bytes.len() < rest_offset {
        return Err(DecodeError::UnexpectedEof);
    }
    let tag = bytes[4];
    Ok((tag, &bytes[5..rest_offset], &bytes[rest_offset..]))
}

/// Read one frame off a blocking stream. An oversized or zero length becomes
/// `InvalidData` — the connection is garbage and the caller should drop it.
pub fn read_frame<R: Read>(r: &mut R) -> io::Result<(u8, Vec<u8>)> {
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes);
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad frame length {len}"),
        ));
    }
    let mut tag = [0u8; 1];
    r.read_exact(&mut tag)?;
    let mut body = vec![0u8; len as usize - 1];
    r.read_exact(&mut body)?;
    Ok((tag[0], body))
}

/// Write one already-framed message (`frame()`'s output) to a stream.
pub fn write_frame<W: Write>(w: &mut W, framed: &[u8]) -> io::Result<()> {
    w.write_all(framed)
}

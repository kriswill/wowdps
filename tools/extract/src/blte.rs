//! BLTE container decoding (the encoding wrapped around every file in CASC
//! storage): `"BLTE"` magic, optional chunk table, then per-chunk payloads
//! tagged with an encoding mode byte.
//!
//! Modes: `N` plain, `Z` zlib, `E` encrypted (Salsa20; the decrypted bytes
//! are again a mode-tagged chunk), `F` recursive BLTE. Encrypted chunks
//! whose key we don't have decode to `logical_size` zero bytes — exactly
//! what the game client does, which is what makes zero-filled encrypted
//! DB2 sections skippable downstream. LZ4 mode `'4'` (12.0+) is rejected
//! loudly until a real file needs it.

use crate::inflate;
use crate::raw;
use crate::salsa20;
use std::collections::HashMap;

/// TACT encryption keys by 8-byte key name (little-endian u64).
pub type Keys = HashMap<u64, [u8; 16]>;

pub fn decode(data: &[u8], keys: &Keys) -> Result<Vec<u8>, String> {
    let header: [u8; 8] =
        raw::array(data, 0, "blte: header").map_err(|_| "blte: too short for header")?;
    if &header[..4] != b"BLTE" {
        return Err("blte: bad magic".into());
    }
    let header_size = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

    if header_size == 0 {
        // Single chunk, no table: the rest is one mode-tagged payload.
        let mut out = Vec::new();
        chunk(
            raw::rest(data, 8, "blte: single chunk")?,
            None,
            0,
            keys,
            &mut out,
        )?;
        return Ok(out);
    }

    let table = data
        .get(8..header_size)
        .ok_or("blte: header size beyond file")?;
    let flags = raw::byte(table, 0, "blte: chunk table flags")?;
    if table.len() < 4 || flags != 0x0F {
        return Err(format!("blte: unsupported chunk table format {flags:#04x}"));
    }
    let count = raw::uint_be(table, 1, 3, "blte: chunk count")? as usize;
    if count == 0 || table.len() < 4 + count * 24 {
        return Err(format!("blte: chunk table truncated ({count} chunks)"));
    }

    let mut out = Vec::new();
    let mut pos = header_size;
    for i in 0..count {
        let entry_off = 4 + i * 24;
        let raw_size = raw::u32_be(table, entry_off, "blte: chunk entry size")? as usize;
        let logical_size =
            raw::u32_be(table, entry_off + 4, "blte: chunk entry logical size")? as usize;
        let payload = raw::take(data, pos, raw_size, "blte: chunk")
            .map_err(|_| format!("blte: chunk {i} truncated"))?;
        pos += raw_size;
        let before = out.len();
        chunk(payload, Some(logical_size), i as u32, keys, &mut out)?;
        if out.len() - before != logical_size {
            return Err(format!(
                "blte: chunk {i} decoded to {} bytes, table says {logical_size}",
                out.len() - before
            ));
        }
    }
    Ok(out)
}

/// Decode one mode-tagged chunk payload (mode byte + data) into `out`.
fn chunk(
    payload: &[u8],
    logical_size: Option<usize>,
    index: u32,
    keys: &Keys,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    let (&mode, body) = payload.split_first().ok_or("blte: empty chunk")?;
    match mode {
        b'N' => {
            out.extend_from_slice(body);
            Ok(())
        }
        b'Z' => {
            out.extend_from_slice(&inflate::zlib(body)?);
            Ok(())
        }
        b'F' => {
            out.extend_from_slice(&decode(body, keys)?);
            Ok(())
        }
        b'E' => {
            let (key_name, iv, cipher) = parse_encrypted(body)?;
            let Some(key) = keys.get(&key_name) else {
                // Unknown key: the client substitutes zeroes of the
                // chunk's logical size and carries on.
                let size =
                    logical_size.ok_or("blte: encrypted single-chunk file with unknown key")?;
                out.resize(out.len() + size, 0);
                return Ok(());
            };
            let mut nonce = [0u8; 8];
            for (slot, &b) in nonce.iter_mut().zip(iv.iter()) {
                *slot = b;
            }
            for (j, b) in nonce.iter_mut().take(4).enumerate() {
                *b ^= (index >> (8 * j)) as u8;
            }
            let mut inner = cipher.to_vec();
            salsa20::apply(key, &nonce, &mut inner);
            chunk(&inner, logical_size, index, keys, out)
        }
        b'4' => Err("blte: LZ4 chunks ('4') not yet supported".into()),
        m => Err(format!("blte: unknown chunk mode {:?}", m as char)),
    }
}

/// `E` chunk header: key name (8 bytes), IV (4 or 8 bytes), cipher type.
fn parse_encrypted(body: &[u8]) -> Result<(u64, &[u8], &[u8]), String> {
    let err = || "blte: malformed encrypted chunk".to_string();
    let (&name_len, rest) = body.split_first().ok_or_else(err)?;
    if name_len != 8 || rest.len() < 8 {
        return Err(format!("blte: unsupported key name length {name_len}"));
    }
    let key_name = raw::u64_le(rest, 0, "blte: key name")?;
    let rest = raw::rest(rest, 8, "blte: encrypted chunk")?;
    let (&iv_len, rest) = rest.split_first().ok_or_else(err)?;
    if !(iv_len == 4 || iv_len == 8) || rest.len() < iv_len as usize + 1 {
        return Err(format!("blte: unsupported IV length {iv_len}"));
    }
    let (iv, rest) = rest.split_at(iv_len as usize);
    let (&cipher_type, cipher) = rest.split_first().ok_or_else(err)?;
    if cipher_type != b'S' {
        return Err(format!(
            "blte: unsupported cipher {:?}",
            cipher_type as char
        ));
    }
    Ok((key_name, iv, cipher))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_keys() -> Keys {
        Keys::new()
    }

    #[test]
    fn single_chunk_plain() {
        let mut f = b"BLTE".to_vec();
        f.extend_from_slice(&0u32.to_be_bytes());
        f.push(b'N');
        f.extend_from_slice(b"payload");
        assert_eq!(decode(&f, &no_keys()).unwrap(), b"payload");
    }

    fn table_entry(raw: usize, logical: usize) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend_from_slice(&(raw as u32).to_be_bytes());
        e.extend_from_slice(&(logical as u32).to_be_bytes());
        e.extend_from_slice(&[0; 16]); // md5, unchecked
        e
    }

    fn multi(chunks: &[(&[u8], usize)]) -> Vec<u8> {
        let header_size = 8 + 4 + chunks.len() * 24;
        let mut f = b"BLTE".to_vec();
        f.extend_from_slice(&(header_size as u32).to_be_bytes());
        f.push(0x0F);
        f.extend_from_slice(&(chunks.len() as u32).to_be_bytes()[1..]);
        for (payload, logical) in chunks {
            f.extend_from_slice(&table_entry(payload.len(), *logical));
        }
        for (payload, _) in chunks {
            f.extend_from_slice(payload);
        }
        f
    }

    #[test]
    fn multi_chunk_plain_and_recursive() {
        let inner = {
            let mut f = b"BLTE".to_vec();
            f.extend_from_slice(&0u32.to_be_bytes());
            f.push(b'N');
            f.extend_from_slice(b"xyz");
            f
        };
        let mut c1 = vec![b'N'];
        c1.extend_from_slice(b"abc");
        let mut c2 = vec![b'F'];
        c2.extend_from_slice(&inner);
        let f = multi(&[(&c1, 3), (&c2, 3)]);
        assert_eq!(decode(&f, &no_keys()).unwrap(), b"abcxyz");
    }

    #[test]
    fn encrypted_roundtrip_and_zero_fill() {
        // Build an encrypted chunk by applying the cipher forward.
        let key = *b"0123456789abcdef";
        let key_name = 0x1122334455667788u64;
        let iv = [9u8, 8, 7, 6];
        let index = 1u32;

        let mut inner = vec![b'N'];
        inner.extend_from_slice(b"secret");
        let mut nonce = [0u8; 8];
        nonce[..4].copy_from_slice(&iv);
        for (j, b) in nonce.iter_mut().take(4).enumerate() {
            *b ^= (index >> (8 * j)) as u8;
        }
        salsa20::apply(&key, &nonce, &mut inner);

        let mut e = vec![b'E', 8];
        e.extend_from_slice(&key_name.to_le_bytes());
        e.push(4);
        e.extend_from_slice(&iv);
        e.push(b'S');
        e.extend_from_slice(&inner);

        let mut c0 = vec![b'N'];
        c0.extend_from_slice(b"lead");
        let f = multi(&[(&c0, 4), (&e, 6)]);

        // Without the key: zero fill.
        assert_eq!(decode(&f, &no_keys()).unwrap(), b"lead\0\0\0\0\0\0");
        // With it: decrypts to the plain payload.
        let mut keys = Keys::new();
        keys.insert(key_name, key);
        assert_eq!(decode(&f, &keys).unwrap(), b"leadsecret");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"BLT", &no_keys()).is_err());
        assert!(decode(b"XXXX\0\0\0\0N", &no_keys()).is_err());
        let mut f = b"BLTE".to_vec();
        f.extend_from_slice(&0u32.to_be_bytes());
        f.push(b'4');
        f.extend_from_slice(b"data");
        assert!(decode(&f, &no_keys()).is_err());
    }
}

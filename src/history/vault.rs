//! Authenticated append-only history container. Callers serialize all operations.
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305,
    aead::{Aead, KeyInit, Payload},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use zeroize::{Zeroize, Zeroizing};
pub const MAX_EVENTS: usize = 100_000;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_VAULT: u64 = 256 * 1024 * 1024;
const HEADER: usize = 56;
const MAGIC: &[u8; 8] = b"TERMSHV1";
const CHECK: &[u8] = b"Terms history authenticated check v1";
#[cfg(target_os = "macos")]
const FLAGS: i32 = 0x100 | 4;
#[cfg(target_os = "linux")]
const FLAGS: i32 = 0x20000 | 0x800;
unsafe extern "C" {
    fn getuid() -> u32;
}
fn error(s: impl std::fmt::Display) -> String {
    format!("history vault: {s}")
}
fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0; N];
    getrandom::fill(&mut b).map_err(error)?;
    Ok(b)
}
pub fn derive(pass: &[u8], salt: &[u8; 16]) -> Result<Zeroizing<Vec<u8>>, String> {
    let params = Params::new(65536, 3, 1, Some(32)).map_err(error)?;
    let mut key = Zeroizing::new(vec![0; 32]);
    let mut memory = Zeroizing::new(vec![argon2::Block::default(); 65536]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(pass, salt, &mut key, memory.as_mut_slice())
        .map_err(error)?;
    Ok(key)
}
fn file(path: &Path, write: bool) -> Result<File, String> {
    let f = OpenOptions::new()
        .read(true)
        .write(write)
        .custom_flags(FLAGS)
        .open(path)
        .map_err(error)?;
    let m = f.metadata().map_err(error)?;
    // SAFETY: getuid has no arguments and no memory preconditions.
    if !m.is_file() || m.uid() != unsafe { getuid() } || m.mode() & 0o777 != 0o600 {
        return Err(error("unsafe file type, owner or permissions"));
    }
    Ok(f)
}
fn bytes(path: &Path) -> Result<Vec<u8>, String> {
    let f = file(path, false)?;
    if f.metadata().map_err(error)?.len() > MAX_VAULT {
        return Err(error("vault exceeds limit"));
    }
    let mut b = Vec::new();
    f.take(MAX_VAULT + 1).read_to_end(&mut b).map_err(error)?;
    if b.len() as u64 > MAX_VAULT {
        return Err(error("vault exceeds limit"));
    }
    Ok(b)
}
fn header(salt: [u8; 16]) -> Result<Vec<u8>, String> {
    let mut h = MAGIC.to_vec();
    h.extend(random::<16>()?);
    h.extend(salt);
    h.extend(65536u32.to_le_bytes());
    h.extend(3u32.to_le_bytes());
    h.extend(1u32.to_le_bytes());
    h.extend(32u32.to_le_bytes());
    Ok(h)
}
fn validate(b: &[u8]) -> Result<(), String> {
    if b.len() < HEADER
        || &b[..8] != MAGIC
        || b[40..44] != 65536u32.to_le_bytes()
        || b[44..48] != 3u32.to_le_bytes()
        || b[48..52] != 1u32.to_le_bytes()
        || b[52..56] != 32u32.to_le_bytes()
    {
        return Err(error("unsupported or truncated header"));
    }
    Ok(())
}
pub fn read_salt(path: &Path) -> Result<[u8; 16], String> {
    let mut b = [0; HEADER];
    file(path, false)?.read_exact(&mut b).map_err(error)?;
    validate(&b)?;
    let mut s = [0; 16];
    s.copy_from_slice(&b[24..40]);
    Ok(s)
}
fn cipher(key: &[u8]) -> Result<XChaCha20Poly1305, String> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| error("invalid key size"))
}
fn aad(h: &[u8], seq: u64, prev: &[u8; 16], len: u32) -> Vec<u8> {
    let mut a = h.to_vec();
    a.extend(seq.to_le_bytes());
    a.extend(prev);
    a.extend(len.to_le_bytes());
    a
}
fn frame(
    c: &XChaCha20Poly1305,
    h: &[u8],
    seq: u64,
    prev: &[u8; 16],
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    if payload.len() > MAX_FRAME {
        return Err(error("frame exceeds limit"));
    }
    let nonce = random::<24>()?;
    let len = (24 + payload.len() + 16) as u32;
    let encrypted = c
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: payload,
                aad: &aad(h, seq, prev, len),
            },
        )
        .map_err(|_| error("encryption failed"))?;
    let mut out = len.to_le_bytes().to_vec();
    out.extend(nonce);
    out.extend(encrypted);
    Ok(out)
}
struct Parsed {
    values: Vec<Vec<u8>>,
    prev: [u8; 16],
    seq: u64,
    end: usize,
    incomplete: bool,
}
fn parse(b: &[u8], key: &[u8], repair: bool) -> Result<Parsed, String> {
    validate(b)?;
    let c = cipher(key)?;
    let h = &b[..HEADER];
    let (mut pos, mut seq, mut prev) = (HEADER, 0, [0; 16]);
    let mut values: Vec<Vec<u8>> = Vec::new();
    let mut total = 0usize;
    let result = (|| {
        while pos < b.len() {
            if seq > MAX_EVENTS as u64 {
                return Err(error("event count exceeds limit"));
            }
            let start = pos;
            if b.len() - pos < 4 {
                if repair && seq > 0 {
                    return Ok(Parsed {
                        values: std::mem::take(&mut values),
                        prev,
                        seq,
                        end: start,
                        incomplete: true,
                    });
                }
                return Err(error("incomplete trailing frame"));
            }
            let len = u32::from_le_bytes(b[pos..pos + 4].try_into().map_err(error)?) as usize;
            if !(40..=MAX_FRAME + 40).contains(&len) {
                return Err(error("invalid frame length"));
            }
            pos += 4;
            if b.len() - pos < len {
                if repair && seq > 0 {
                    return Ok(Parsed {
                        values: std::mem::take(&mut values),
                        prev,
                        seq,
                        end: start,
                        incomplete: true,
                    });
                }
                return Err(error("incomplete trailing frame"));
            }
            let mut nonce = [0; 24];
            nonce.copy_from_slice(&b[pos..pos + 24]);
            let mut plain = c
                .decrypt(
                    (&nonce).into(),
                    Payload {
                        msg: &b[pos + 24..pos + len],
                        aad: &aad(h, seq, &prev, len as u32),
                    },
                )
                .map_err(|_| error("wrong key or authentication failure"))?;
            if seq == 0 {
                let valid = plain == CHECK;
                plain.zeroize();
                if !valid {
                    return Err(error("invalid check frame"));
                }
            } else {
                total = total
                    .checked_add(plain.len())
                    .ok_or_else(|| error("index exceeds limit"))?;
                if total > 64 * 1024 * 1024 {
                    plain.zeroize();
                    return Err(error("index exceeds limit"));
                }
                values.push(plain);
            }
            prev.copy_from_slice(&b[pos + len - 16..pos + len]);
            pos += len;
            seq = seq
                .checked_add(1)
                .ok_or_else(|| error("sequence overflow"))?;
        }
        if seq == 0 {
            return Err(error("missing authenticated check"));
        }
        Ok(Parsed {
            values: std::mem::take(&mut values),
            prev,
            seq,
            end: pos,
            incomplete: false,
        })
    })();
    if result.is_err() {
        for v in &mut values {
            v.zeroize();
        }
    }
    result
}
fn sync_parent(path: &Path) -> Result<(), String> {
    File::open(path.parent().ok_or_else(|| error("missing parent"))?)
        .and_then(|f| f.sync_all())
        .map_err(error)
}
pub fn init(path: &Path, pass: &[u8]) -> Result<(), String> {
    let salt = random::<16>()?;
    let key = derive(pass, &salt)?;
    let h = header(salt)?;
    let check = frame(&cipher(&key)?, &h, 0, &[0; 16], CHECK)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(FLAGS)
        .open(path)
        .map_err(error)?;
    f.write_all(&h)
        .and_then(|_| f.write_all(&check))
        .and_then(|_| f.sync_all())
        .map_err(error)?;
    sync_parent(path)
}
pub fn load(path: &Path, key: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    Ok(parse(&bytes(path)?, key, false)?.values)
}
pub fn append(path: &Path, key: &[u8], payload: &[u8]) -> Result<(), String> {
    let b = bytes(path)?;
    let mut p = parse(&b, key, false)?;
    if p.seq > MAX_EVENTS as u64 {
        for v in &mut p.values {
            v.zeroize();
        }
        return Err(error("event count exceeds limit"));
    }
    let total: usize = p.values.iter().map(Vec::len).sum();
    for v in &mut p.values {
        v.zeroize();
    }
    if total.saturating_add(payload.len()) > 64 * 1024 * 1024 {
        return Err(error("index exceeds limit"));
    }
    let f = frame(&cipher(key)?, &b[..HEADER], p.seq, &p.prev, payload)?;
    if b.len() as u64 + f.len() as u64 > MAX_VAULT {
        return Err(error("vault exceeds limit"));
    }
    let mut out = file(path, true)?;
    use std::io::{Seek, SeekFrom};
    out.seek(SeekFrom::End(0)).map_err(error)?;
    out.write_all(&f)
        .and_then(|_| out.sync_all())
        .map_err(error)
}
pub fn rewrite(path: &Path, key: &[u8], payloads: &[Vec<u8>]) -> Result<(), String> {
    if payloads.len() > MAX_EVENTS {
        return Err(error("event count exceeds limit"));
    }
    if payloads
        .iter()
        .try_fold(0usize, |n, p| n.checked_add(p.len()))
        .is_none_or(|n| n > 64 * 1024 * 1024)
    {
        return Err(error("index exceeds limit"));
    }
    let b = bytes(path)?;
    let mut verified = parse(&b, key, false)?;
    for v in &mut verified.values {
        v.zeroize();
    }
    let mut salt = [0; 16];
    salt.copy_from_slice(&b[24..40]);
    let h = header(salt)?;
    let c = cipher(key)?;
    let mut output = h.clone();
    let mut prev = [0; 16];
    for (seq, payload) in std::iter::once(CHECK)
        .chain(payloads.iter().map(Vec::as_slice))
        .enumerate()
    {
        let f = frame(&c, &h, seq as u64, &prev, payload)?;
        if output.len() as u64 + f.len() as u64 > MAX_VAULT {
            return Err(error("vault exceeds limit"));
        }
        prev.copy_from_slice(&f[f.len() - 16..]);
        output.extend(f);
    }
    let name = format!(
        ".history-{}.tmp",
        random::<16>()?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let tmp = path
        .parent()
        .ok_or_else(|| error("missing parent"))?
        .join(name);
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(FLAGS)
            .open(&tmp)
            .map_err(error)?;
        f.write_all(&output)
            .and_then(|_| f.sync_all())
            .map_err(error)?;
        fs::rename(&tmp, path).map_err(error)?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub fn repair(path: &Path, key: &[u8]) -> Result<(), String> {
    let b = bytes(path)?;
    let mut p = parse(&b, key, true)?;
    for v in &mut p.values {
        v.zeroize();
    }
    if p.incomplete {
        let f = file(path, true)?;
        f.set_len(p.end as u64)
            .and_then(|_| f.sync_all())
            .map_err(error)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_roundtrip() {
        let dir = std::env::temp_dir().join(format!("terms-vault-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vault");
        let _ = fs::remove_file(&path);
        init(&path, b"test passphrase").unwrap();
        let key = derive(b"test passphrase", &read_salt(&path).unwrap()).unwrap();
        assert!(load(&path, &[9; 32]).is_err());
        append(&path, &key, b"secret").unwrap();
        assert_eq!(load(&path, &key).unwrap(), vec![b"secret".to_vec()]);
        rewrite(&path, &key, &[b"kept".to_vec()]).unwrap();
        assert_eq!(load(&path, &key).unwrap(), vec![b"kept".to_vec()]);
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&[1, 2]).unwrap();
        assert!(load(&path, &key).is_err());
        repair(&path, &key).unwrap();
        assert_eq!(load(&path, &key).unwrap(), vec![b"kept".to_vec()]);
        let mut b = bytes(&path).unwrap();
        *b.last_mut().unwrap() ^= 1;
        fs::write(&path, b).unwrap();
        assert!(load(&path, &key).is_err());
        assert!(repair(&path, &key).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}

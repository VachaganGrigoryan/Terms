//! Private, transactional directory navigation state.
use std::{
    cmp::Ordering,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const LIMIT: u64 = 64 * 1024 * 1024;
const MAGIC: &[u8] = b"TERMSJMP\x01";
#[cfg(target_os = "macos")]
const NOFOLLOW: i32 = 0x100;
#[cfg(target_os = "linux")]
const NOFOLLOW: i32 = 0x20000;
#[cfg(target_os = "macos")]
const NONBLOCK: i32 = 4;
#[cfg(target_os = "linux")]
const NONBLOCK: i32 = 0x800;
unsafe extern "C" {
    fn getuid() -> u32;
    fn flock(fd: i32, operation: i32) -> i32;
}
#[derive(Clone, Debug)]
struct Entry {
    path: Vec<u8>,
    visits: u64,
    last: u64,
}
#[derive(Default)]
struct Store {
    entries: Vec<Entry>,
    excludes: Vec<Vec<u8>>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn uid() -> u32 {
    // SAFETY: getuid has no arguments or memory preconditions.
    unsafe { getuid() }
}
fn err(e: impl std::fmt::Display) -> String {
    format!("jump: {e}")
}
fn root() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        let p = PathBuf::from(p);
        if !p.is_absolute() {
            return Err(err("XDG_STATE_HOME must be absolute"));
        }
        Ok(p.join("terms"))
    } else {
        Ok(
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| err("HOME unavailable"))?)
                .join(".local/state/terms"),
        )
    }
}
fn check(meta: &fs::Metadata, dir: bool) -> Result<(), String> {
    if meta.uid() != uid()
        || meta.permissions().mode() & 0o077 != 0
        || if dir { !meta.is_dir() } else { !meta.is_file() }
    {
        return Err(err("unsafe state ownership, permissions or file type"));
    }
    Ok(())
}
fn directory(root: &Path, create: bool) -> Result<bool, String> {
    match fs::symlink_metadata(root) {
        Ok(m) => {
            check(&m, true)?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create {
                return Ok(false);
            }
            if let Some(p) = root.parent() {
                fs::create_dir_all(p).map_err(err)?;
            }
            match fs::DirBuilder::new().mode(0o700).create(root) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(err(e)),
            }
            check(&fs::symlink_metadata(root).map_err(err)?, true)?;
            Ok(true)
        }
        Err(e) => Err(err(e)),
    }
}
use std::os::unix::fs::DirBuilderExt;
fn open(path: &Path, write: bool, create: bool) -> Result<File, String> {
    let f = OpenOptions::new()
        .read(true)
        .write(write)
        .create(create)
        .mode(0o600)
        .custom_flags(NOFOLLOW | NONBLOCK)
        .open(path)
        .map_err(err)?;
    check(&f.metadata().map_err(err)?, false)?;
    Ok(f)
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        // SAFETY: live descriptor, flock unlock takes no pointers.
        unsafe {
            flock(self.0.as_raw_fd(), 8);
        }
    }
}
fn lock(root: &Path) -> Result<Lock, String> {
    let f = open(&root.join("jump.lock"), true, true)?;
    let start = Instant::now();
    loop {
        // SAFETY: descriptor remains valid throughout the native call.
        if unsafe { flock(f.as_raw_fd(), 2 | 4) } == 0 {
            return Ok(Lock(f));
        }
        if start.elapsed() > Duration::from_secs(2) {
            return Err(err("state lock timeout"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn num(bytes: &[u8], p: &mut usize) -> Result<u64, String> {
    let end = p.checked_add(8).ok_or_else(|| err("corrupt store"))?;
    let b = bytes.get(*p..end).ok_or_else(|| err("truncated store"))?;
    *p = end;
    Ok(u64::from_le_bytes(b.try_into().map_err(err)?))
}
fn path(bytes: &[u8], p: &mut usize) -> Result<Vec<u8>, String> {
    let n = num(bytes, p)? as usize;
    if n == 0 || n > 16384 {
        return Err(err("invalid path length"));
    }
    let end = p.checked_add(n).ok_or_else(|| err("corrupt store"))?;
    let b = bytes.get(*p..end).ok_or_else(|| err("truncated path"))?;
    *p = end;
    if b.contains(&0) || !b.starts_with(b"/") {
        return Err(err("invalid stored path"));
    }
    Ok(b.to_vec())
}
fn decode(bytes: &[u8]) -> Result<Store, String> {
    if !bytes.starts_with(MAGIC) {
        return Err(err("unsupported or corrupt store"));
    }
    let mut p = MAGIC.len();
    let n = num(bytes, &mut p)?;
    let x = num(bytes, &mut p)?;
    if n > 10000 || x > 1000 {
        return Err(err("store count limit"));
    }
    let mut s = Store::default();
    for _ in 0..n {
        let path = path(bytes, &mut p)?;
        if s.entries.iter().any(|e| e.path == path) {
            return Err(err("duplicate path"));
        }
        s.entries.push(Entry {
            path,
            visits: num(bytes, &mut p)?,
            last: num(bytes, &mut p)?,
        });
    }
    for _ in 0..x {
        s.excludes.push(path(bytes, &mut p)?);
    }
    if p != bytes.len() {
        return Err(err("trailing store data"));
    }
    Ok(s)
}
fn read(root: &Path) -> Result<Store, String> {
    let p = root.join("jump.db");
    if !p.try_exists().map_err(err)? {
        if fs::symlink_metadata(&p).is_ok() {
            return Err(err("invalid state link"));
        }
        return Ok(Store::default());
    }
    let mut bytes = Vec::new();
    open(&p, false, false)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > LIMIT {
        return Err(err("store size limit"));
    }
    decode(&bytes)
}
fn encode(s: &Store) -> Vec<u8> {
    let mut b = MAGIC.to_vec();
    b.extend((s.entries.len() as u64).to_le_bytes());
    b.extend((s.excludes.len() as u64).to_le_bytes());
    let put = |b: &mut Vec<u8>, p: &[u8]| {
        b.extend((p.len() as u64).to_le_bytes());
        b.extend(p);
    };
    for e in &s.entries {
        put(&mut b, &e.path);
        b.extend(e.visits.to_le_bytes());
        b.extend(e.last.to_le_bytes());
    }
    for p in &s.excludes {
        put(&mut b, p)
    }
    b
}
fn save(root: &Path, s: &Store) -> Result<(), String> {
    let bytes = encode(s);
    if bytes.len() as u64 > LIMIT {
        return Err(err("store size limit"));
    }
    let mut chosen = None;
    for i in 0..100 {
        let p = root.join(format!(".jump-{}-{i}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(NOFOLLOW | NONBLOCK)
            .open(&p)
        {
            Ok(f) => {
                chosen = Some((p, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(err(e)),
        }
    }
    let (p, mut f) = chosen.ok_or_else(|| err("temporary file collisions"))?;
    let result = (|| {
        f.write_all(&bytes).map_err(err)?;
        f.sync_all().map_err(err)?;
        fs::rename(&p, root.join("jump.db")).map_err(err)?;
        File::open(root).and_then(|f| f.sync_all()).map_err(err)
    })();
    if result.is_err() {
        let _ = fs::remove_file(p);
    }
    result
}
fn canonical(p: &std::ffi::OsStr) -> Result<Vec<u8>, String> {
    let p = fs::canonicalize(p).map_err(err)?;
    if !p.is_dir() {
        return Err(err("not a directory"));
    }
    let b = p.as_os_str().as_bytes();
    if b.len() > 16384 {
        return Err(err("path too long"));
    }
    Ok(b.to_vec())
}
fn under(p: &[u8], root: &[u8]) -> bool {
    p == root || p.starts_with(root) && (root == b"/" || p.get(root.len()) == Some(&b'/'))
}
fn order(a: &Entry, b: &Entry, t: u64) -> Ordering {
    let ad = 1 + t.saturating_sub(a.last) / 86400;
    let bd = 1 + t.saturating_sub(b.last) / 86400;
    (u128::from(b.visits) * u128::from(ad))
        .cmp(&(u128::from(a.visits) * u128::from(bd)))
        .then_with(|| b.last.cmp(&a.last))
        .then_with(|| a.path.len().cmp(&b.path.len()))
        .then_with(|| a.path.cmp(&b.path))
}
fn matches(p: &[u8], q: &[u8]) -> bool {
    let case = q.iter().any(u8::is_ascii_uppercase);
    q.is_empty()
        || p.windows(q.len()).any(|w| {
            w.iter().zip(q).all(|(a, b)| {
                if case {
                    a == b
                } else {
                    a.eq_ignore_ascii_case(b)
                }
            })
        })
}
fn emit(records: &[Vec<u8>], out: &mut dyn Write, null: bool, hex: bool) -> Result<(), String> {
    if hex && records.iter().map(Vec::len).sum::<usize>() > 1024 * 1024 {
        return Err(err("shell output limit"));
    }
    for r in records {
        if hex {
            for b in r {
                write!(out, "{b:02x}").map_err(err)?;
            }
            out.write_all(b"\n").map_err(err)?;
        } else if null {
            out.write_all(r).map_err(err)?;
            out.write_all(&[0]).map_err(err)?;
        } else {
            for &b in r {
                if (32..127).contains(&b) && b != b'\\' {
                    out.write_all(&[b]).map_err(err)?;
                } else {
                    write!(out, "\\x{b:02x}").map_err(err)?;
                }
            }
            out.write_all(b"\n").map_err(err)?;
        }
    }
    Ok(())
}
/// Execute directory-state operations. Read-only queries never create missing state.
pub fn execute(args: &[OsString], out: &mut dyn Write) -> Result<u8, String> {
    execute_at(args, out, &root()?)
}
fn execute_at(args: &[OsString], out: &mut dyn Write, root: &Path) -> Result<u8, String> {
    let cmd = args
        .first()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("missing operation"))?;
    let modifying = matches!(cmd, "add" | "remove" | "clear")
        || cmd == "exclude" && args.get(1).is_some_and(|x| x == "add" || x == "remove");
    if !directory(root, modifying)? {
        return Ok(1);
    }
    let _lock = if modifying { Some(lock(root)?) } else { None };
    let mut s = read(root)?;
    let t = now();
    match cmd {
        "add" | "remove" => {
            if args.len() != 2 {
                return Err(err("expected one path"));
            }
            let p = if cmd == "add" {
                canonical(&args[1])?
            } else {
                let p = PathBuf::from(&args[1]);
                if !p.is_absolute() {
                    canonical(&args[1])?
                } else {
                    p.into_os_string().into_vec()
                }
            };
            if cmd == "remove" {
                s.entries.retain(|e| e.path != p);
            } else if !s.excludes.iter().any(|x| under(&p, x)) {
                if let Some(e) = s.entries.iter_mut().find(|e| e.path == p) {
                    e.visits = e.visits.saturating_add(1);
                    e.last = t;
                } else {
                    s.entries.push(Entry {
                        path: p,
                        visits: 1,
                        last: t,
                    });
                }
                s.entries.sort_by(|a, b| order(a, b, t));
                s.entries.truncate(10000);
            }
            save(root, &s)?;
            Ok(0)
        }
        "clear" => {
            if args.len() != 1 {
                return Err(err("clear takes no arguments"));
            }
            s.entries.clear();
            save(root, &s)?;
            Ok(0)
        }
        "exclude" => match args.get(1).and_then(|s| s.to_str()) {
            Some("list") if args.len() == 2 => {
                emit(&s.excludes, out, false, false)?;
                Ok(0)
            }
            Some("add" | "remove") if args.len() == 3 => {
                let p = if args[1] == "remove" && Path::new(&args[2]).is_absolute() {
                    args[2].clone().into_vec()
                } else {
                    canonical(&args[2])?
                };
                if args[1] == "add" {
                    if !s.excludes.contains(&p) {
                        if s.excludes.len() >= 1000 {
                            return Err(err("exclusion limit"));
                        }
                        s.excludes.push(p.clone());
                    }
                    s.entries.retain(|e| !under(&e.path, &p));
                } else {
                    s.excludes.retain(|x| x != &p);
                }
                save(root, &s)?;
                Ok(0)
            }
            _ => Err(err("invalid exclusion operation")),
        },
        "query" | "list" => {
            let (mut null, mut hex, mut interactive, mut options) = (false, false, false, true);
            let mut terms = Vec::new();
            for a in &args[1..] {
                if options && a == "--" {
                    options = false;
                } else if options && a == "--null" {
                    null = true;
                } else if options && a == "--hex" {
                    hex = true;
                } else if options && a == "--interactive" && cmd == "query" {
                    interactive = true;
                } else if options && a.as_bytes().starts_with(b"-") {
                    return Err(err("unknown query option"));
                } else {
                    terms.push(a.as_bytes().to_vec());
                }
            }
            if null && hex || cmd == "list" && !terms.is_empty() {
                return Err(err("conflicting query arguments"));
            }
            s.entries.retain(|e| {
                Path::new(&OsString::from_vec(e.path.clone())).is_dir()
                    && terms.iter().all(|q| matches(&e.path, q))
            });
            s.entries.sort_by(|a, b| order(a, b, t));
            let mut records: Vec<_> = s.entries.into_iter().map(|e| e.path).collect();
            if records.is_empty() {
                return Ok(1);
            }
            if interactive {
                records = match crate::pick::choose(records, "", false, true)? {
                    Some(v) => v,
                    None => return Ok(130),
                }
            } else if cmd == "query" {
                records.truncate(1)
            }
            emit(&records, out, null, hex)?;
            Ok(0)
        }
        _ => Err(err("unknown jump operation")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_and_ranking() {
        let mut s = Store::default();
        s.entries.push(Entry {
            path: b"/tmp/a\n".to_vec(),
            visits: 3,
            last: 2,
        });
        assert_eq!(decode(&encode(&s)).unwrap().entries[0].path, b"/tmp/a\n");
        assert!(decode(b"bad").is_err());
        let a = Entry {
            path: b"/a".to_vec(),
            visits: 2,
            last: 0,
        };
        let b = Entry {
            path: b"/b".to_vec(),
            visits: 1,
            last: 0,
        };
        assert_eq!(order(&a, &b, 100), Ordering::Less);
        assert!(under(b"/a/b", b"/a"));
        assert!(!under(b"/abc", b"/a"));
    }
    #[test]
    fn corruption_and_symlink_rejected() {
        let p = std::env::temp_dir().join(format!("terms-jump-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        directory(&p, true).unwrap();
        let db = p.join("jump.db");
        fs::write(&db, b"bad").unwrap();
        fs::set_permissions(&db, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read(&p).is_err());
        fs::remove_file(&db).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", &db).unwrap();
        assert!(read(&p).is_err());
        fs::remove_dir_all(&p).unwrap();
    }
    #[test]
    fn concurrent_updates_and_exclusions() {
        let p = std::env::temp_dir().join(format!("terms-jump-concurrent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        directory(&p, true).unwrap();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let p = p.clone();
                std::thread::spawn(move || {
                    execute_at(&["add".into(), "/tmp".into()], &mut Vec::new(), &p).unwrap()
                })
            })
            .collect();
        for thread in threads {
            assert_eq!(thread.join().unwrap(), 0);
        }
        let s = read(&p).unwrap();
        assert_eq!(s.entries[0].visits, 8);
        execute_at(
            &["exclude".into(), "add".into(), "/tmp".into()],
            &mut Vec::new(),
            &p,
        )
        .unwrap();
        execute_at(&["add".into(), "/tmp".into()], &mut Vec::new(), &p).unwrap();
        assert!(read(&p).unwrap().entries.is_empty());
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn state_roundtrip() {
        let p = std::env::temp_dir().join(format!("terms-jump-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        let mut out = Vec::new();
        assert_eq!(execute_at(&["list".into()], &mut out, &p).unwrap(), 1);
        assert!(!p.exists());
        execute_at(&["add".into(), "/tmp".into()], &mut out, &p).unwrap();
        execute_at(&["query".into(), "--hex".into()], &mut out, &p).unwrap();
        assert!(!out.is_empty());
        assert_eq!(
            fs::metadata(p.join("jump.db")).unwrap().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(p).unwrap();
    }
}

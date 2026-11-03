use super::{model::*, vault};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};
const LIMIT: usize = 1024 * 1024;
#[cfg(target_os = "macos")]
const NOFOLLOW: i32 = 0x100;
#[cfg(target_os = "linux")]
const NOFOLLOW: i32 = 0x20000;
unsafe extern "C" {
    fn getuid() -> u32;
    fn flock(fd: i32, op: i32) -> i32;
    #[cfg(target_os = "macos")]
    fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
    #[cfg(target_os = "linux")]
    fn getsockopt(
        fd: i32,
        level: i32,
        name: i32,
        value: *mut std::ffi::c_void,
        len: *mut u32,
    ) -> i32;
    #[cfg(target_os = "macos")]
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        size: i32,
    ) -> i32;
    fn listen(fd: i32, backlog: i32) -> i32;
}
pub fn uid() -> u32 {
    // SAFETY: getuid has no pointer parameters.
    unsafe { getuid() }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn root() -> Result<PathBuf, String> {
    let base = if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(p)
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or("HOME not set")?).join(".local/state")
    };
    if !base.is_absolute() {
        return Err("history state directory must be absolute".into());
    }
    Ok(base.join("terms/history"))
}
pub fn secure_root(root: &Path, create: bool) -> Result<(), String> {
    if create && !root.exists() {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let m = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != uid() || m.mode() & 0o077 != 0 {
        return Err("unsafe history directory ownership/type/permissions".into());
    }
    Ok(())
}
fn peer(s: &UnixStream) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let good = {
        let (mut u, mut g) = (0, 0); // SAFETY: writable initialized uid/gid and live socket.
        unsafe { getpeereid(s.as_raw_fd(), &mut u, &mut g) == 0 && u == uid() }
    };
    #[cfg(target_os = "linux")]
    let good = {
        let mut c = [0i32; 3];
        let mut len = 12u32; // SAFETY: ucred occupies three 32-bit fields; writable valid buffer.
        unsafe {
            getsockopt(s.as_raw_fd(), 1, 17, c.as_mut_ptr().cast(), &mut len) == 0
                && len == 12
                && c[1] as u32 == uid()
        }
    };
    if good {
        Ok(())
    } else {
        Err("history peer credentials rejected".into())
    }
}
pub fn identity(pid: u32) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        #[repr(C)]
        struct Bsd {
            fields: [u32; 12],
            names: [u8; 48],
            details: [u32; 6],
            start: [u64; 2],
        }
        let mut b = Bsd {
            fields: [0; 12],
            names: [0; 48],
            details: [0; 6],
            start: [0; 2],
        }; // SAFETY: layout matches proc_bsdinfo in Darwin SDK; initialized live buffer.
        let n = unsafe {
            proc_pidinfo(
                pid as i32,
                3,
                0,
                (&mut b as *mut Bsd).cast(),
                std::mem::size_of::<Bsd>() as i32,
            )
        };
        if n != std::mem::size_of::<Bsd>() as i32 || b.fields[5] != uid() {
            return Err("shell process is unavailable or has different owner".into());
        }
        Ok(format!("{}:{}", b.start[0], b.start[1]))
    }
    #[cfg(target_os = "linux")]
    {
        let path = format!("/proc/{pid}");
        if fs::metadata(&path).map_err(|_| "shell unavailable")?.uid() != uid() {
            return Err("shell owner mismatch".into());
        }
        let stat = fs::read_to_string(format!("{path}/stat")).map_err(|_| "shell unavailable")?;
        let tail = stat.rsplit_once(')').ok_or("invalid proc stat")?.1;
        let start = tail
            .split_whitespace()
            .nth(19)
            .ok_or("invalid proc start")?;
        Ok(start.to_string())
    }
}
pub fn random_id() -> Result<String, String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|_| "randomness failed")?;
    Ok(hex(&b))
}
fn transfer_read(s: &mut UnixStream, mut b: &mut [u8], deadline: Instant) -> Result<(), String> {
    while !b.is_empty() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or("history request deadline exceeded")?;
        s.set_read_timeout(Some(left.max(Duration::from_millis(1))))
            .map_err(|e| e.to_string())?;
        match s.read(b) {
            Ok(0) => return Err("history frame truncated".into()),
            Ok(n) => b = &mut b[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err("history request deadline or read failure".into()),
        }
    }
    Ok(())
}
fn read_frame(s: &mut UnixStream) -> Result<Zeroizing<Vec<u8>>, String> {
    read_frame_timeout(s, Duration::from_secs(1))
}
fn read_frame_timeout(s: &mut UnixStream, budget: Duration) -> Result<Zeroizing<Vec<u8>>, String> {
    let deadline = Instant::now() + budget;
    let mut n = [0; 4];
    transfer_read(s, &mut n, deadline)?;
    let n = u32::from_be_bytes(n) as usize;
    if n > LIMIT {
        return Err("history frame exceeds limit".into());
    }
    let mut b = Zeroizing::new(vec![0; n]);
    transfer_read(s, &mut b, deadline)?;
    Ok(b)
}
fn write_frame(s: &mut UnixStream, b: &[u8]) -> Result<(), String> {
    if b.len() > LIMIT {
        return Err("history response exceeds limit".into());
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    let header = (b.len() as u32).to_be_bytes();
    for mut slice in [header.as_slice(), b] {
        while !slice.is_empty() {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or("history write deadline exceeded")?;
            s.set_write_timeout(Some(left.max(Duration::from_millis(1))))
                .map_err(|e| e.to_string())?;
            match s.write(slice) {
                Ok(0) => return Err("history transport closed".into()),
                Ok(n) => slice = &slice[n..],
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err("history write deadline or failure".into()),
            }
        }
    }
    Ok(())
}

pub fn request(root: &Path, op: &str, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    secure_root(root, false)?;
    let socket = root.join("agent.sock");
    let m = fs::symlink_metadata(&socket)
        .map_err(|_| "history agent unavailable; run history unlock")?;
    use std::os::unix::fs::FileTypeExt;
    if !m.file_type().is_socket() || m.uid() != uid() || m.mode() & 0o077 != 0 {
        return Err("unsafe history socket".into());
    }
    let mut s = UnixStream::connect(socket).map_err(|_| "history agent unavailable")?;
    peer(&s)?;
    s.set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let mut b = Zeroizing::new(format!("TH1\n{op}\n").into_bytes());
    b.extend_from_slice(data);
    write_frame(&mut s, &b)?;
    let mut reply = read_frame_timeout(&mut s, Duration::from_secs(15))?;
    if reply.first() != Some(&0) {
        return Err(String::from_utf8_lossy(reply.get(1..).unwrap_or_default()).into());
    }
    reply.remove(0);
    Ok(reply)
}
struct Session {
    pid: u32,
    identity: String,
    pending: Option<(String, Instant)>,
}
struct State {
    key: Option<Zeroizing<Vec<u8>>>,
    model: Model,
    sessions: HashMap<String, Session>,
    activity: Instant,
    seen_session: bool,
    unlock_at: Option<Instant>,
}
impl State {
    fn lock(&mut self) {
        self.key = None;
        self.model = Model::default();
        for s in self.sessions.values_mut() {
            s.pending = None;
        }
    }
    fn persist(&mut self, path: &Path, event: &[u8]) -> Result<(), String> {
        let key = self
            .key
            .as_ref()
            .ok_or("history locked; recording skipped")?;
        let mut next = self.model.clone();
        next.apply(event)?;
        let before = next.records.len();
        next.prune(now());
        let compact =
            before != next.records.len() || event.starts_with(b"delete\t") || event == b"clear";
        let result = if compact {
            let mut e = next.events();
            let result = vault::rewrite(path, key, &e);
            for b in &mut e {
                b.zeroize();
            }
            result
        } else {
            vault::append(path, key, event)
        };
        result?;
        self.model = next;
        self.activity = Instant::now();
        Ok(())
    }
    fn handle(&mut self, path: &Path, op: &str, data: &[u8]) -> Result<Vec<u8>, String> {
        if self.activity.elapsed() >= Duration::from_secs(900) {
            self.lock();
        }
        let text = std::str::from_utf8(data).map_err(|_| "invalid history request")?;
        let f: Vec<_> = text.lines().collect();
        match op {
            "status" => {
                return Ok(format!(
                    "{}\n",
                    if self.key.is_some() {
                        "unlocked"
                    } else {
                        "locked"
                    }
                )
                .into_bytes());
            }
            "lock" => {
                self.lock();
                return Ok(Vec::new());
            }
            "unlock" => {
                if self
                    .unlock_at
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                {
                    return Err("unlock attempts are rate limited".into());
                }
                self.unlock_at = Some(Instant::now());
                self.lock();
                let key = Zeroizing::new(unhex(text.trim())?);
                if key.len() != 32 {
                    return Err("invalid key".into());
                }
                let mut events = vault::load(path, &key)?;
                let mut model = Model::default();
                let mut result = Ok(());
                for e in &mut events {
                    if result.is_ok() {
                        result = model.apply(e)
                    }
                    e.zeroize();
                }
                result?;
                self.key = Some(key);
                self.model = model;
                self.activity = Instant::now();
                return Ok(Vec::new());
            }
            "repair-key" => {
                let key = Zeroizing::new(unhex(text.trim())?);
                if key.len() != 32 {
                    return Err("invalid key".into());
                }
                self.lock();
                vault::repair(path, &key)?;
                return Ok(Vec::new());
            }
            "session-open" => {
                let pid = number(text.trim())?;
                let identity = identity(pid)?;
                if self.sessions.len() >= 128 {
                    return Err("session limit".into());
                }
                let id = random_id()?;
                self.sessions.insert(
                    id.clone(),
                    Session {
                        pid,
                        identity,
                        pending: None,
                    },
                );
                self.seen_session = true;
                return Ok(format!("{id}\n").into_bytes());
            }
            "session-close" => {
                self.sessions.remove(text.trim());
                if self.sessions.is_empty() {
                    self.lock()
                }
                return Ok(Vec::new());
            }
            _ => {}
        }
        if self.key.is_none() {
            return Err("history locked; recording skipped".into());
        }
        self.activity = Instant::now();
        match op {
            "begin" if f.len() == 3 => {
                let session = self.sessions.get(f[0]).ok_or("unknown shell session")?;
                if identity(session.pid)? != session.identity {
                    return Err("stale shell session".into());
                }
                if session.pending.is_some() {
                    return Err("command already pending".into());
                }
                let cwd = Zeroizing::new(unhex(f[1])?);
                let command = Zeroizing::new(unhex(f[2])?);
                if self.model.excluded(&command, &cwd) {
                    return Ok(b"excluded\n".to_vec());
                }
                let id = random_id()?;
                let event = Zeroizing::new(
                    format!("start\t{id}\t{}\t{}\t{}\t{}", f[0], f[1], f[2], now()).into_bytes(),
                );
                self.persist(path, &event)?;
                self.sessions
                    .get_mut(f[0])
                    .ok_or("session missing")?
                    .pending = Some((id.clone(), Instant::now()));
                Ok(format!("{id}\n").into_bytes())
            }
            "finish" if f.len() == 3 => {
                let session = self.sessions.get(f[0]).ok_or("unknown shell session")?;
                if identity(session.pid)? != session.identity {
                    return Err("stale shell session".into());
                }
                let p = self
                    .sessions
                    .get(f[0])
                    .and_then(|s| s.pending.as_ref())
                    .ok_or("no pending command")?;
                if p.0 != f[1] {
                    return Err("command identity mismatch".into());
                }
                let exit: i32 = number(f[2])?;
                let event = format!("finish\t{}\t{}\t{exit}", p.0, p.1.elapsed().as_millis());
                self.persist(path, event.as_bytes())?;
                self.sessions
                    .get_mut(f[0])
                    .ok_or("session missing")?
                    .pending = None;
                Ok(Vec::new())
            }
            "config" => {
                let mut b = Zeroizing::new(Vec::with_capacity(LIMIT));
                writeln!(b, "retention\t{}\t{}", self.model.days, self.model.max)
                    .map_err(|e| e.to_string())?;
                for (kind, list) in [("prefix", &self.model.prefixes), ("dir", &self.model.dirs)] {
                    for v in list {
                        if b.len() + kind.len() + v.len() * 2 + 2 > LIMIT - 1 {
                            return Err("config exceeds response limit".into());
                        }
                        writeln!(b, "{kind}\t{}", Hex(v)).map_err(|e| e.to_string())?;
                    }
                }
                Ok(std::mem::take(&mut *b))
            }
            "import" => {
                let mut candidate = Model::default();
                candidate.apply(data)?;
                let r = candidate.records.first().ok_or("invalid imported start")?;
                if self.model.excluded(&r.command, &r.cwd) {
                    return Ok(b"excluded".to_vec());
                }
                self.persist(path, data)?;
                Ok(Vec::new())
            }
            "records" => {
                let mut output = Zeroizing::new(Vec::with_capacity(LIMIT));
                for r in &self.model.records {
                    let line = Zeroizing::new(format!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                        r.id,
                        r.session,
                        Hex(&r.command),
                        Hex(&r.cwd),
                        r.time,
                        r.duration.map(|x| x.to_string()).unwrap_or_default(),
                        r.exit.map(|x| x.to_string()).unwrap_or_default()
                    ));
                    if output.len() + line.len() > LIMIT - 1 {
                        return Err("history response exceeds 1 MiB; reduce retention".into());
                    }
                    output.extend(line.as_bytes());
                }
                Ok(std::mem::take(&mut *output))
            }
            "event" => {
                self.persist(path, data)?;
                Ok(Vec::new())
            }
            "repair" => {
                vault::repair(path, self.key.as_ref().ok_or("locked")?)?;
                Ok(Vec::new())
            }
            _ => Err("unsupported history operation".into()),
        }
    }
}
pub fn serve(root: &Path) -> Result<(), String> {
    secure_root(root, true)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(NOFOLLOW | nonblock())
        .open(root.join("agent.lock"))
        .map_err(|e| e.to_string())?;
    let m = lock.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != uid() || m.mode() & 0o077 != 0 {
        return Err("unsafe agent lock".into());
    } // SAFETY: live descriptor and pointer-free flock operation.
    if unsafe { flock(lock.as_raw_fd(), 2 | 4) } != 0 {
        return Err("history agent already running".into());
    }
    let socket = root.join("agent.sock");
    if let Ok(m) = fs::symlink_metadata(&socket) {
        use std::os::unix::fs::FileTypeExt;
        if !m.file_type().is_socket() || m.uid() != uid() || m.mode() & 0o077 != 0 {
            return Err("unsafe stale socket".into());
        }
        if UnixStream::connect(&socket).is_ok() {
            return Err("history agent already responding".into());
        }
        fs::remove_file(&socket).map_err(|e| e.to_string())?;
    }
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?; // SAFETY: live listener descriptor, bounded backlog.
    if unsafe { listen(listener.as_raw_fd(), 32) } != 0 {
        return Err("cannot bound listener backlog".into());
    }
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut state = State {
        key: None,
        model: Model::default(),
        sessions: HashMap::new(),
        activity: Instant::now(),
        seen_session: false,
        unlock_at: None,
    };
    let mut liveness = Instant::now();
    loop {
        if liveness.elapsed() >= Duration::from_secs(5) {
            state
                .sessions
                .retain(|_, s| identity(s.pid).is_ok_and(|id| id == s.identity));
            if state.seen_session && state.sessions.is_empty()
                || state.activity.elapsed() >= Duration::from_secs(900)
            {
                state.lock()
            }
            liveness = Instant::now();
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .map_err(|e| e.to_string())?;
                let result = (|| {
                    peer(&stream)?;
                    let request = read_frame(&mut stream)?;
                    let split = request
                        .iter()
                        .position(|b| *b == b'\n')
                        .ok_or("missing protocol")?;
                    if &request[..split] != b"TH1" {
                        return Err("unsupported history protocol".into());
                    }
                    let rest = &request[split + 1..];
                    let n = rest
                        .iter()
                        .position(|b| *b == b'\n')
                        .ok_or("missing operation")?;
                    let op = std::str::from_utf8(&rest[..n]).map_err(|_| "invalid operation")?;
                    state.handle(&root.join("vault.bin"), op, &rest[n + 1..])
                })();
                let mut reply = Zeroizing::new(Vec::new());
                match result {
                    Ok(mut b) => {
                        reply.push(0);
                        {
                            let b = Zeroizing::new(std::mem::take(&mut b));
                            reply.extend_from_slice(&b)
                        }
                    }
                    Err(e) => {
                        reply.push(2);
                        reply.extend(e.as_bytes())
                    }
                }
                let _ = write_frame(&mut stream, &reply);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}
#[cfg(target_os = "macos")]
fn nonblock() -> i32 {
    4
}
#[cfg(target_os = "linux")]
fn nonblock() -> i32 {
    0x800
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peer_and_framing() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        peer(&a).unwrap();
        write_frame(&mut a, b"TH1\nstatus\n").unwrap();
        assert_eq!(read_frame(&mut b).unwrap().as_slice(), b"TH1\nstatus\n");
    }
    #[test]
    fn capture_lock_and_exclusions() {
        let root =
            std::env::temp_dir().join(format!("terms-history-state-{}", random_id().unwrap()));
        secure_root(&root, true).unwrap();
        let path = root.join("vault.bin");
        vault::init(&path, b"test-passphrase").unwrap();
        let key = vault::derive(b"test-passphrase", &vault::read_salt(&path).unwrap()).unwrap();
        let mut s = State {
            key: None,
            model: Model::default(),
            sessions: HashMap::new(),
            activity: Instant::now(),
            seen_session: false,
            unlock_at: None,
        };
        s.handle(&path, "unlock", hex(&key).as_bytes()).unwrap();
        let id = String::from_utf8(
            s.handle(
                &path,
                "session-open",
                std::process::id().to_string().as_bytes(),
            )
            .unwrap(),
        )
        .unwrap();
        let id = id.trim();
        let data = format!("{id}\n{}\n{}\n", hex(b"/tmp"), hex(b" ls"));
        assert_eq!(
            s.handle(&path, "begin", data.as_bytes()).unwrap(),
            b"excluded\n"
        );
        let data = format!("{id}\n{}\n{}\n", hex(b"/tmp"), hex(b"ls"));
        let cmd = String::from_utf8(s.handle(&path, "begin", data.as_bytes()).unwrap()).unwrap();
        assert!(s.model.records[0].exit.is_none());
        s.handle(
            &path,
            "finish",
            format!("{id}\n{}\n7\n", cmd.trim()).as_bytes(),
        )
        .unwrap();
        assert_eq!(s.model.records[0].exit, Some(7));
        s.handle(&path, "lock", b"").unwrap();
        assert!(s.model.records.is_empty());
        assert!(s.key.is_none());
        assert!(s.handle(&path, "records", b"").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn absolute_deadline_rejects_trickled_frame() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            for b in [0u8, 0, 0, 4, b'a', b'b'] {
                if writer.write_all(&[b]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(80));
            }
        });
        let start = Instant::now();
        assert!(read_frame_timeout(&mut reader, Duration::from_millis(150)).is_err());
        assert!(start.elapsed() < Duration::from_millis(400));
        drop(reader);
        worker.join().unwrap();
    }
    #[test]
    fn repair_is_available_after_restart() {
        let root =
            std::env::temp_dir().join(format!("terms-history-repair-{}", random_id().unwrap()));
        secure_root(&root, true).unwrap();
        let path = root.join("vault.bin");
        vault::init(&path, b"test-password").unwrap();
        let key = vault::derive(b"test-password", &vault::read_salt(&path).unwrap()).unwrap();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[0, 0])
            .unwrap();
        assert!(vault::load(&path, &key).is_err());
        let mut state = State {
            key: None,
            model: Model::default(),
            sessions: HashMap::new(),
            activity: Instant::now(),
            seen_session: false,
            unlock_at: None,
        };
        state
            .handle(&path, "repair-key", hex(&key).as_bytes())
            .unwrap();
        assert!(vault::load(&path, &key).is_ok());
        assert!(state.key.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}

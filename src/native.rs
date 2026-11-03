//! Small Unix terminal boundary. Constants/layouts match the target platform headers.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicI32, Ordering};
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct Termios {
    input: u64,
    output: u64,
    control: u64,
    local: u64,
    cc: [u8; 20],
    ispeed: u64,
    ospeed: u64,
}
#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
struct Termios {
    input: u32,
    output: u32,
    control: u32,
    local: u32,
    line: u8,
    cc: [u8; 32],
    ispeed: u32,
    ospeed: u32,
}
#[cfg(target_os = "linux")]
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
#[repr(C)]
#[derive(Default)]
struct Size {
    rows: u16,
    columns: u16,
    x: u16,
    y: u16,
}
#[cfg(target_os = "linux")]
type Nfds = usize;
unsafe extern "C" {
    fn tcgetattr(fd: i32, p: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, action: i32, p: *const Termios) -> i32;
    fn cfmakeraw(p: *mut Termios);
    fn tcgetpgrp(fd: i32) -> i32;
    fn getpgrp() -> i32;
    #[cfg(target_os = "linux")]
    fn poll(fds: *mut PollFd, n: Nfds, timeout: i32) -> i32;
    fn ioctl(fd: i32, request: usize, ...) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
    fn raise(sig: i32) -> i32;
}
static EVENTS: AtomicI32 = AtomicI32::new(0);
#[cfg(target_os = "macos")]
const SUSPEND: i32 = 18;
#[cfg(target_os = "linux")]
const SUSPEND: i32 = 20;
extern "C" fn notify(sig: i32) {
    EVENTS.fetch_or(
        if sig == SUSPEND {
            2
        } else if sig == 28 {
            4
        } else {
            1
        },
        Ordering::Relaxed,
    );
}
pub struct Terminal {
    file: File,
    saved: Termios,
    raw: Termios,
    handlers: Vec<(i32, usize)>,
}
impl Terminal {
    pub fn open() -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|e| format!("interactive command requires /dev/tty: {e}"))?;
        let fd = file.as_raw_fd();
        // SAFETY: fd is live; tcgetattr initializes the platform ABI value before use.
        let saved = unsafe {
            if tcgetpgrp(fd) != getpgrp() {
                return Err("interactive command requires foreground terminal".into());
            }
            let mut t = std::mem::MaybeUninit::uninit();
            if tcgetattr(fd, t.as_mut_ptr()) != 0 {
                return Err("cannot read terminal settings".into());
            }
            t.assume_init()
        };
        let mut raw = saved;
        // SAFETY: initialized exclusive termios pointer and live descriptor.
        unsafe {
            cfmakeraw(&mut raw);
            if tcsetattr(fd, 0, &raw) != 0 {
                return Err("cannot set terminal settings".into());
            }
        }
        let mut result = Self {
            file,
            saved,
            raw,
            handlers: Vec::new(),
        };
        EVENTS.store(0, Ordering::Relaxed);
        for sig in [2, 15, 1, 28, SUSPEND] {
            // SAFETY: notify is an extern handler performing only lock-free atomic notification.
            let previous = unsafe { signal(sig, notify as *const () as usize) };
            if previous == usize::MAX {
                return Err("cannot install terminal signal handler".into());
            }
            result.handlers.push((sig, previous));
        }
        result.write(b"\x1b[?1049h\x1b[?25l")?;
        Ok(result)
    }
    pub fn write(&mut self, b: &[u8]) -> Result<(), String> {
        self.file
            .write_all(b)
            .and_then(|_| self.file.flush())
            .map_err(|_| "terminal write failed".into())
    }
    pub fn dimensions(&self) -> (usize, usize) {
        let mut s = Size::default();
        #[cfg(target_os = "macos")]
        let request = 0x40087468usize;
        #[cfg(target_os = "linux")]
        let request = 0x5413usize;
        // SAFETY: Size matches winsize and fd is live.
        unsafe {
            ioctl(self.file.as_raw_fd(), request, &mut s);
        }
        (usize::from(s.rows).max(1), usize::from(s.columns).max(1))
    }
    pub fn wait(&mut self, stdin: bool, busy: bool) -> Result<(bool, bool, bool), String> {
        let e = EVENTS.swap(0, Ordering::Relaxed);
        if e & 1 != 0 {
            return Ok((false, false, true));
        }
        if e & 2 != 0 {
            self.write(b"\x1b[?25h\x1b[?1049l")?;
            // SAFETY: restore live settings before normal suspension; reinstall upon continuation.
            unsafe {
                if tcsetattr(self.file.as_raw_fd(), 0, &self.saved) != 0 {
                    return Err("cannot restore terminal before suspension".into());
                }
                signal(SUSPEND, 0);
                raise(SUSPEND);
                signal(SUSPEND, notify as *const () as usize);
                if tcsetattr(self.file.as_raw_fd(), 0, &self.raw) != 0 {
                    return Err("cannot resume raw terminal".into());
                }
            }
            self.write(b"\x1b[?1049h\x1b[?25l")?;
        }
        #[cfg(target_os = "macos")]
        {
            mac_readiness(self.file.as_raw_fd(), stdin, busy)
        }
        #[cfg(target_os = "linux")]
        {
            let mut fds = [
                PollFd {
                    fd: self.file.as_raw_fd(),
                    events: 1,
                    revents: 0,
                },
                PollFd {
                    fd: if stdin { 0 } else { -1 },
                    events: 1,
                    revents: 0,
                },
            ];
            // SAFETY: valid array of two initialized pollfd values.
            let r = unsafe { poll(fds.as_mut_ptr(), 2, if busy { 0 } else { 50 }) };
            if r < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return Err("terminal poll failed".into());
            }
            if r < 0 {
                return Ok((false, false, false));
            }
            if fds[0].revents & (8 | 16 | 32) != 0 {
                return Err(format!(
                    "terminal readiness failed (poll flags {}); leaving selector",
                    fds[0].revents
                ));
            }
            if fds[1].revents & (8 | 32) != 0 {
                return Err("candidate input readiness failed".into());
            }
            Ok((fds[0].revents & 1 != 0, fds[1].revents & 17 != 0, false))
        }
    }

    pub fn suspend(&mut self) {
        EVENTS.fetch_or(2, Ordering::Relaxed);
    }
    pub fn key(&mut self) -> Result<Vec<u8>, String> {
        let mut b = [0; 64];
        let read = self.file.read(&mut b);
        let result = match read {
            Ok(0) => Err("terminal input closed".into()),
            Ok(n) => Ok(b[..n].to_vec()),
            Err(_) => Err("terminal read failed".into()),
        };
        // This buffer can contain history passphrase bytes as well as UI keys.
        zeroize::Zeroize::zeroize(&mut b);
        result
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.file.write_all(b"\x1b[?25h\x1b[?1049l");
        // SAFETY: saved settings and previous handlers were captured from this process; descriptor remains live.
        unsafe {
            tcsetattr(self.file.as_raw_fd(), 0, &self.saved);
            for &(s, h) in &self.handlers {
                signal(s, h);
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct FdSet {
    bits: [u32; 32],
}
#[cfg(target_os = "macos")]
#[repr(C)]
struct Timeval {
    seconds: i64,
    microseconds: i32,
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    #[link_name = "select"]
    fn select_read(
        n: i32,
        read: *mut FdSet,
        write: *mut FdSet,
        error: *mut FdSet,
        timeout: *mut Timeval,
    ) -> i32;
}
#[cfg(target_os = "macos")]
fn mac_readiness(terminal: i32, input: bool, busy: bool) -> Result<(bool, bool, bool), String> {
    if !(0..1024).contains(&terminal) {
        return Err("terminal descriptor exceeds select capacity".into());
    }
    let mut read = FdSet { bits: [0; 32] };
    let fd = terminal as usize;
    read.bits[fd / 32] |= 1u32 << (fd % 32);
    if input {
        read.bits[0] |= 1;
    }
    let mut timeout = Timeval {
        seconds: 0,
        microseconds: if busy { 0 } else { 50_000 },
    };
    // SAFETY: layouts verified against Darwin SDK fd_set/timeval. Every fd is
    // bounded below FD_SETSIZE; all pointers are initialized, exclusive and live.
    let result = unsafe {
        select_read(
            terminal + 1,
            &mut read,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut timeout,
        )
    };
    if result < 0 {
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return Ok((false, false, false));
        }
        return Err(format!(
            "terminal select failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((
        read.bits[fd / 32] & (1u32 << (fd % 32)) != 0,
        input && read.bits[0] & 1 != 0,
        false,
    ))
}
#[cfg(all(test, target_os = "macos"))]
mod readiness_tests {
    use super::*;
    #[test]
    fn select_detects_pipe_data_and_eof() {
        use std::os::unix::net::UnixStream;
        let (reader, mut writer) = UnixStream::pair().unwrap();
        assert!(!mac_readiness(reader.as_raw_fd(), false, true).unwrap().0);
        writer.write_all(b"x").unwrap();
        assert!(mac_readiness(reader.as_raw_fd(), false, true).unwrap().0);
        drop(writer);
        assert!(mac_readiness(reader.as_raw_fd(), false, true).unwrap().0);
        assert!(mac_readiness(1024, false, true).is_err());
    }
}

//! Local encrypted command history. No activation occurs merely by invoking Terms.
mod agent;
mod model;
mod vault;
use model::{Hex, hex, number, unhex};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
use zeroize::Zeroizing;
fn passphrase(label: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut terminal = crate::native::Terminal::open()?;
    terminal.write(label.as_bytes())?;
    let mut pass = Zeroizing::new(Vec::with_capacity(4096));
    loop {
        let (keys, _, cancel) = terminal.wait(false, false)?;
        if cancel {
            return Err("passphrase entry cancelled".into());
        }
        if keys {
            let batch = Zeroizing::new(terminal.key()?);
            for &b in batch.iter() {
                match b {
                    3 | 27 => return Err("passphrase entry cancelled".into()),
                    b'\r' | b'\n' => {
                        if pass.is_empty() {
                            return Err("empty passphrase refused".into());
                        }
                        return Ok(pass);
                    }
                    8 | 127 => {
                        pass.pop();
                    }
                    b if b >= 32 => {
                        if pass.len() >= 4096 {
                            return Err("passphrase exceeds limit".into());
                        }
                        pass.push(b)
                    }
                    _ => {}
                }
            }
        }
    }
}
fn start_agent(root: &Path) -> Result<(), String> {
    if agent::request(root, "status", b"").is_ok() {
        return Ok(());
    }
    Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args(["history", "agent"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    for _ in 0..100 {
        if agent::request(root, "status", b"").is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("history agent did not start; run terms history agent to inspect error".into())
}
fn bounded(input: &mut dyn Read) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut b = Zeroizing::new(Vec::new());
    input
        .take(1024 * 1024 + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    if b.len() > 1024 * 1024 {
        return Err("history input exceeds limit".into());
    }
    Ok(b)
}
fn write(out: &mut dyn Write, b: &[u8]) -> Result<(), String> {
    out.write_all(b).map_err(|e| e.to_string())
}
fn escaped(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .chars()
        .flat_map(char::escape_default)
        .collect()
}
fn records(root: &Path) -> Result<Vec<model::Record>, String> {
    let data = agent::request(root, "records", b"")?;
    let text = std::str::from_utf8(&data).map_err(|_| "invalid agent response")?;
    let mut result = Vec::new();
    for l in text.lines() {
        let f: Vec<_> = l.split('\t').collect();
        if f.len() != 7 {
            return Err("invalid record".into());
        }
        result.push(model::Record {
            id: f[0].into(),
            session: f[1].into(),
            command: unhex(f[2])?,
            cwd: unhex(f[3])?,
            time: number(f[4])?,
            duration: if f[5].is_empty() {
                None
            } else {
                Some(number(f[5])?)
            },
            exit: if f[6].is_empty() {
                None
            } else {
                Some(number(f[6])?)
            },
        })
    }
    Ok(result)
}
fn event(root: &Path, event: &str) -> Result<(), String> {
    agent::request(root, "event", event.as_bytes())?;
    Ok(())
}
fn json_field(line: &[u8], field: &str) -> Result<Zeroizing<String>, String> {
    let mut input = std::io::Cursor::new(line);
    let mut out = Zeroizing::new(Vec::new());
    crate::json::execute(
        &["-r".into(), format!(".{field}").into()],
        &mut input,
        &mut *out,
        false,
    )?;
    let s = std::str::from_utf8(&out).map_err(|_| "invalid import field")?;
    Ok(Zeroizing::new(
        s.strip_suffix('\n').unwrap_or(s).to_string(),
    ))
}
/// Execute explicitly requested history operations; sensitive payloads use stdin/IPC.
pub fn execute(
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    terminal: bool,
) -> Result<u8, String> {
    match execute_internal(args, input, out, terminal) {
        Err(e) if e == "passphrase entry cancelled" => Ok(130),
        other => other,
    }
}
fn execute_internal(
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    _terminal: bool,
) -> Result<u8, String> {
    let cmd = args
        .first()
        .and_then(|a| a.to_str())
        .ok_or("missing history operation")?;
    let rest = &args[1..];
    let root = agent::root()?;
    match cmd {
        "agent" if rest.is_empty() => {
            agent::serve(&root)?;
            Ok(0)
        }
        "init" if rest.is_empty() => {
            agent::secure_root(&root, true)?;
            let pass = passphrase("New history passphrase: ")?;
            let again = passphrase("Confirm history passphrase: ")?;
            if *pass != *again {
                return Err("passphrases differ".into());
            }
            vault::init(&root.join("vault.bin"), &pass)?;
            Ok(0)
        }
        "unlock" if rest.is_empty() => {
            agent::secure_root(&root, false)?;
            let salt = vault::read_salt(&root.join("vault.bin"))?;
            let pass = passphrase("History passphrase: ")?;
            let key = vault::derive(&pass, &salt)?;
            start_agent(&root)?;
            let encoded = Zeroizing::new(hex(&key));
            agent::request(&root, "unlock", encoded.as_bytes())?;
            Ok(0)
        }
        "lock" | "status" if rest.is_empty() => {
            if cmd == "status" && !root.exists() {
                write(out, b"uninitialized\n")?;
                return Ok(0);
            }
            match agent::request(&root, cmd, b"") {
                Ok(reply) => {
                    write(out, &reply)?;
                    Ok(0)
                }
                Err(e) if cmd == "status" => {
                    write(out, b"agent unavailable\n")?;
                    Err(e)
                }
                Err(e) => Err(e),
            }
        }
        "session-open" | "session-close" | "begin" | "finish" if rest.is_empty() => {
            let data = bounded(input)?;
            let reply = agent::request(&root, cmd, &data)?;
            write(out, &reply)?;
            Ok(0)
        }
        "search" => {
            let mut cwd = None;
            let mut session = None;
            let mut exit = None;
            let mut terms = Vec::new();
            let mut i = 0;
            let mut options = true;
            while i < rest.len() {
                let arg = rest[i].to_str().ok_or("search options must be UTF-8")?;
                if options && arg == "--" {
                    options = false
                } else if options && matches!(arg, "--cwd" | "--session" | "--exit") {
                    i += 1;
                    let v = rest.get(i).ok_or("missing search option value")?;
                    match arg {
                        "--cwd" => cwd = Some(v.as_os_str().as_bytes().to_vec()),
                        "--session" => {
                            session = Some(v.to_str().ok_or("invalid session")?.to_string())
                        }
                        _ => exit = Some(number::<i32>(v.to_str().ok_or("invalid exit")?)?),
                    }
                } else if options && arg.starts_with('-') {
                    return Err("unknown history search option".into());
                } else {
                    terms.push(arg.as_bytes().to_vec())
                }
                i += 1;
            }
            let mut count = 0;
            for r in records(&root)? {
                if cwd.as_ref().is_some_and(|x| *x != r.cwd)
                    || session.as_ref().is_some_and(|x| *x != r.session)
                    || exit.is_some_and(|x| Some(x) != r.exit)
                {
                    continue;
                }
                if !terms.iter().all(|q| {
                    let fold = !q.iter().any(u8::is_ascii_uppercase);
                    q.is_empty()
                        || r.command.windows(q.len()).any(|w| {
                            w.iter().zip(q).all(|(a, b)| {
                                if fold {
                                    a.eq_ignore_ascii_case(b)
                                } else {
                                    a == b
                                }
                            })
                        })
                }) {
                    continue;
                }
                write(
                    out,
                    format!(
                        "{}\t{}\t{}\t{}\t{}\t{}\n",
                        r.id,
                        r.time,
                        r.duration
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| "incomplete".into()),
                        r.exit.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
                        escaped(&r.cwd),
                        escaped(&r.command)
                    )
                    .as_bytes(),
                )?;
                count += 1;
            }
            Ok(if count == 0 { 1 } else { 0 })
        }
        "pick" => {
            if !(rest.is_empty() || rest == [OsString::from("--hex")]) {
                return Err("history pick accepts only --hex".into());
            }
            let all = records(&root)?;
            let candidates = all.iter().map(|r| r.command.clone()).collect();
            let selected = crate::pick::choose(candidates, "", false, true)?;
            let Some(values) = selected else {
                return Ok(130);
            };
            if values.is_empty() {
                return Ok(1);
            }
            for value in values {
                let value = Zeroizing::new(value);
                if rest.is_empty() {
                    write(out, &value)?;
                    write(out, b"\n")?
                } else {
                    let text = Zeroizing::new(hex(&value));
                    write(out, text.as_bytes())?;
                    write(out, b"\n")?
                }
            }
            Ok(0)
        }
        "delete" if rest.len() == 1 => {
            let id = rest[0].to_str().ok_or("invalid command ID")?;
            if !id.bytes().all(|b| b.is_ascii_hexdigit()) || id.len() != 32 {
                return Err("invalid command ID".into());
            }
            event(&root, &format!("delete\t{id}"))?;
            Ok(0)
        }
        "clear" if rest == [OsString::from("--all")] => {
            event(&root, "clear")?;
            Ok(0)
        }
        "retention" => {
            let config = agent::request(&root, "config", b"")?;
            let text = std::str::from_utf8(&config).map_err(|_| "invalid config")?;
            let fields: Vec<_> = text
                .lines()
                .next()
                .ok_or("missing retention")?
                .split('\t')
                .collect();
            if fields.len() != 3 {
                return Err("invalid retention response".into());
            }
            let (mut days, mut max) = (number::<u64>(fields[1])?, number::<usize>(fields[2])?);
            if rest.is_empty() {
                let reply = agent::request(&root, "config", b"")?;
                write(out, &reply)?;
                return Ok(0);
            }
            let mut i = 0;
            while i < rest.len() {
                let arg = rest[i].to_str().ok_or("invalid retention")?;
                i += 1;
                let v = rest
                    .get(i)
                    .and_then(|v| v.to_str())
                    .ok_or("missing retention value")?;
                match arg {
                    "--days" => days = number(v)?,
                    "--max-commands" => max = number(v)?,
                    _ => return Err("unknown retention option".into()),
                }
                i += 1
            }
            event(&root, &format!("retention\t{days}\t{max}"))?;
            Ok(0)
        }
        "exclude" => {
            if rest == [OsString::from("list")] {
                let reply = agent::request(&root, "config", b"")?;
                write(out, &reply)?;
                return Ok(0);
            }
            if rest.len() != 3 {
                return Err("usage: history exclude add|remove prefix|dir VALUE".into());
            }
            let action = rest[0].to_str().ok_or("invalid exclusion")?;
            let kind = rest[1].to_str().ok_or("invalid exclusion")?;
            if !matches!(action, "add" | "remove") || !matches!(kind, "prefix" | "dir") {
                return Err("invalid exclusion".into());
            }
            let value = if kind == "dir" {
                fs::canonicalize(&rest[2])
                    .map_err(|e| e.to_string())?
                    .as_os_str()
                    .as_bytes()
                    .to_vec()
            } else {
                rest[2].as_os_str().as_bytes().to_vec()
            };
            let encoded = Zeroizing::new(hex(&value));
            event(
                &root,
                &format!("exclude\t{kind}\t{action}\t{}", encoded.as_str()),
            )?;
            Ok(0)
        }
        "repair" if rest == [OsString::from("--truncate-incomplete-tail")] => {
            agent::secure_root(&root, false)?;
            let salt = vault::read_salt(&root.join("vault.bin"))?;
            let pass = passphrase("History repair passphrase: ")?;
            let key = vault::derive(&pass, &salt)?;
            start_agent(&root)?;
            let encoded = Zeroizing::new(hex(&key));
            agent::request(&root, "repair-key", encoded.as_bytes())?;
            Ok(0)
        }
        "export" if rest.len() == 2 && rest[0] == "--plaintext" => {
            let records = records(&root)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&rest[1])
                .map_err(|e| e.to_string())?;
            for r in records {
                let line = Zeroizing::new(format!(
                    "{{\"command_hex\":\"{}\",\"cwd_hex\":\"{}\",\"time\":\"{}\",\"duration\":\"{}\",\"exit\":\"{}\"}}\n",
                    Hex(&r.command),
                    Hex(&r.cwd),
                    r.time,
                    r.duration.map(|x| x.to_string()).unwrap_or_default(),
                    r.exit.map(|x| x.to_string()).unwrap_or_default()
                ));
                file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            }
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(0)
        }
        "import" if rest.len() == 1 => {
            #[cfg(target_os = "macos")]
            let flags = 0x100 | 4;
            #[cfg(target_os = "linux")]
            let flags = 0x20000 | 0x800;
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(&rest[0])
                .map_err(|e| e.to_string())?;
            if !file.metadata().map_err(|e| e.to_string())?.is_file() {
                return Err("import requires regular file".into());
            }
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(file);
            loop {
                let mut bytes = Zeroizing::new(Vec::new());
                let n = reader
                    .by_ref()
                    .take(1024 * 1024 + 1)
                    .read_until(b'\n', &mut bytes)
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                if n > 1024 * 1024 {
                    return Err("import line exceeds limit".into());
                }
                let command = Zeroizing::new(unhex(&json_field(&bytes, "command_hex")?)?);
                let cwd = Zeroizing::new(unhex(&json_field(&bytes, "cwd_hex")?)?);
                let time: u64 = number(&json_field(&bytes, "time")?)?;
                let duration = json_field(&bytes, "duration")?;
                let exit = json_field(&bytes, "exit")?;
                let id = agent::random_id()?;
                let line = Zeroizing::new(format!(
                    "start\t{id}\timport\t{}\t{}\t{time}",
                    Hex(&cwd),
                    Hex(&command)
                ));
                let result = agent::request(&root, "import", line.as_bytes())?;
                if result.as_slice() == b"excluded" {
                    continue;
                }
                if !duration.is_empty() && !exit.is_empty() {
                    let d: u64 = number(&duration)?;
                    let e: i32 = number(&exit)?;
                    event(&root, &format!("finish\t{id}\t{d}\t{e}"))?;
                }
            }
            Ok(0)
        }
        _ => Err("unknown history operation or invalid arguments".into()),
    }
}

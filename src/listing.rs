//! Deterministic, read-only file listings with escaped display names.
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

const HELP: &str = "Usage: terms ls [--all] [--long] [--tree] [--level N] [--sort name|size|modified] [--reverse] [--header] [--color auto|never|always] [--] [PATH...]\nOne entry per line; symlinks are displayed, never intentionally followed.\nLong columns: TYPE MODE UID GID BYTES MODIFIED NAME\n";
const MAX_ENTRIES: usize = 1_000_000;
const MAX_NAMES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Options {
    all: bool,
    long: bool,
    tree: bool,
    level: Option<usize>,
    sort: Sort,
    reverse: bool,
    header: bool,
    color: bool,
    paths: Vec<PathBuf>,
}
#[derive(Default)]
enum Sort {
    #[default]
    Name,
    Size,
    Modified,
}
struct Entry {
    path: PathBuf,
    name: OsString,
    metadata: Metadata,
}

/// List paths without changing filesystem contents or following directory links.
pub fn execute(args: &[OsString], out: &mut dyn Write, terminal: bool) -> Result<u8, String> {
    if args.len() == 1 && args[0] == "--help" {
        out.write_all(HELP.as_bytes())
            .map_err(|_| "cannot write listing help".to_string())?;
        return Ok(0);
    }
    let options = parse(args, terminal)?;
    let mut errors = Vec::new();
    if options.header {
        writeln!(out, "TYPE MODE UID GID BYTES MODIFIED NAME").map_err(write_error)?;
    }
    for path in &options.paths {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                record_error(&mut errors, path_error(path, &error))?;
                continue;
            }
        };
        if options.tree {
            let entry = Entry {
                path: path.clone(),
                name: path.as_os_str().to_owned(),
                metadata,
            };
            if let Err(error) = show(&entry, "", &options, out) {
                record_error(&mut errors, error)?;
            }
            if entry.metadata.is_dir() {
                walk(path, 0, "", &options, out, &mut errors)?;
            }
        } else if metadata.is_dir() {
            if options.paths.len() > 1 {
                writeln!(out, "{}:", escape(path.as_os_str().as_bytes())).map_err(write_error)?;
            }
            match entries(path, &options) {
                Ok(entries) => {
                    for entry in entries {
                        if let Err(error) = show(&entry, "", &options, out) {
                            record_error(&mut errors, error)?;
                        }
                    }
                }
                Err(error) => record_error(&mut errors, error)?,
            }
        } else {
            let entry = Entry {
                path: path.clone(),
                name: path.as_os_str().to_owned(),
                metadata,
            };
            if let Err(error) = show(&entry, "", &options, out) {
                record_error(&mut errors, error)?;
            }
        }
    }
    if errors.is_empty() {
        Ok(0)
    } else {
        Err(errors.join("\n"))
    }
}
fn record_error(errors: &mut Vec<String>, error: String) -> Result<(), String> {
    if error == "cannot write listing output" {
        return Err(error);
    }
    if errors.len() < 64 {
        errors.push(error);
    } else if errors.len() == 64 {
        errors.push("additional listing errors omitted".into());
    }
    Ok(())
}
fn write_error(_: std::io::Error) -> String {
    "cannot write listing output".into()
}
fn path_error(path: &Path, error: &std::io::Error) -> String {
    format!(
        "{}: {}",
        escape(path.as_os_str().as_bytes()),
        escape(error.to_string().as_bytes())
    )
}
fn parse(args: &[OsString], terminal: bool) -> Result<Options, String> {
    let mut options = Options {
        color: terminal
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var_os("TERM").is_some_and(|v| v != "dumb"),
        ..Options::default()
    };
    let mut end = false;
    let mut pos = 0;
    while pos < args.len() {
        let arg = &args[pos];
        if !end && arg == "--" {
            end = true;
        } else if !end && arg.as_bytes().starts_with(b"-") {
            match arg.to_str() {
                Some("--all") => options.all = true,
                Some("--long") => options.long = true,
                Some("--tree") => options.tree = true,
                Some("--reverse") => options.reverse = true,
                Some("--header") => options.header = true,
                Some(flag @ ("--level" | "--sort" | "--color")) => {
                    pos += 1;
                    let value = args
                        .get(pos)
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| format!("{flag} requires a value"))?;
                    match flag {
                        "--level" => {
                            let level = value
                                .parse::<usize>()
                                .map_err(|_| "--level requires an integer from 0 to 256")?;
                            if level > 256 {
                                return Err("--level exceeds 256".into());
                            }
                            options.level = Some(level);
                        }
                        "--sort" => {
                            options.sort = match value {
                                "name" => Sort::Name,
                                "size" => Sort::Size,
                                "modified" => Sort::Modified,
                                _ => return Err("--sort requires name, size, or modified".into()),
                            }
                        }
                        _ => {
                            options.color = match value {
                                "auto" => {
                                    terminal
                                        && std::env::var_os("NO_COLOR").is_none()
                                        && std::env::var_os("TERM").is_some_and(|v| v != "dumb")
                                }
                                "always" => true,
                                "never" => false,
                                _ => return Err("--color requires auto, never, or always".into()),
                            }
                        }
                    }
                }
                _ => return Err("unknown listing option; use terms ls --help".into()),
            }
        } else {
            options.paths.push(PathBuf::from(arg));
        }
        pos += 1;
    }
    if options.header && !options.long {
        return Err("--header requires --long".into());
    }
    if options.level.is_some() && !options.tree {
        return Err("--level requires --tree".into());
    }
    if options.paths.is_empty() {
        options.paths.push(PathBuf::from("."));
    }
    Ok(options)
}
fn entries(path: &Path, options: &Options) -> Result<Vec<Entry>, String> {
    let iterator = fs::read_dir(path).map_err(|error| path_error(path, &error))?;
    let mut result = Vec::new();
    let mut name_bytes = 0usize;
    for item in iterator {
        let item = item.map_err(|error| path_error(path, &error))?;
        let name = item.file_name();
        if !options.all && name.as_bytes().starts_with(b".") {
            continue;
        }
        name_bytes = name_bytes
            .checked_add(name.as_bytes().len())
            .ok_or("listing name budget exceeded")?;
        if result.len() >= MAX_ENTRIES || name_bytes > MAX_NAMES {
            return Err("listing directory budget exceeded".into());
        }
        let path = item.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| path_error(&path, &error))?;
        result.push(Entry {
            path,
            name,
            metadata,
        });
    }
    result.sort_by(|a, b| {
        let primary = match options.sort {
            Sort::Name => std::cmp::Ordering::Equal,
            Sort::Size => a.metadata.len().cmp(&b.metadata.len()),
            Sort::Modified => a
                .metadata
                .mtime()
                .cmp(&b.metadata.mtime())
                .then(a.metadata.mtime_nsec().cmp(&b.metadata.mtime_nsec())),
        };
        primary.then(a.name.as_bytes().cmp(b.name.as_bytes()))
    });
    if options.reverse {
        result.reverse();
    }
    Ok(result)
}
fn walk(
    path: &Path,
    depth: usize,
    prefix: &str,
    options: &Options,
    out: &mut dyn Write,
    errors: &mut Vec<String>,
) -> Result<(), String> {
    if depth >= options.level.unwrap_or(256) {
        return Ok(());
    }
    let children = match entries(path, options) {
        Ok(v) => v,
        Err(error) => {
            record_error(errors, error)?;
            return Ok(());
        }
    };
    let count = children.len();
    for (index, child) in children.into_iter().enumerate() {
        let last = index + 1 == count;
        let branch = format!("{prefix}{}", if last { "`-- " } else { "|-- " });
        if let Err(error) = show(&child, &branch, options, out) {
            record_error(errors, error)?;
        }
        if child.metadata.is_dir() {
            if depth + 1 == 256 && options.level.is_none() {
                record_error(
                    errors,
                    format!(
                        "{}: tree depth limit reached",
                        escape(child.path.as_os_str().as_bytes())
                    ),
                )?;
            } else {
                let next = format!("{prefix}{}", if last { "    " } else { "|   " });
                walk(&child.path, depth + 1, &next, options, out, errors)?;
            }
        }
    }
    Ok(())
}
fn show(entry: &Entry, prefix: &str, options: &Options, out: &mut dyn Write) -> Result<(), String> {
    let file_type = entry.metadata.file_type();
    let kind = if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else if file_type.is_file() {
        '-'
    } else if file_type.is_fifo() {
        'p'
    } else if file_type.is_socket() {
        's'
    } else if file_type.is_block_device() {
        'b'
    } else if file_type.is_char_device() {
        'c'
    } else {
        '?'
    };
    let mut line = String::new();
    if options.long {
        let timestamp = timestamp(entry.metadata.mtime())?;
        line.push_str(&format!(
            "{kind} {:04o} {} {} {} {timestamp} ",
            entry.metadata.mode() & 0o7777,
            entry.metadata.uid(),
            entry.metadata.gid(),
            entry.metadata.len()
        ));
    }
    line.push_str(prefix);
    if options.color {
        line.push_str(if file_type.is_dir() {
            "\x1b[34m"
        } else if file_type.is_symlink() {
            "\x1b[36m"
        } else {
            "\x1b[0m"
        });
    }
    line.push_str(&escape(entry.name.as_bytes()));
    if options.color {
        line.push_str("\x1b[0m");
    }
    if file_type.is_symlink() {
        let target = fs::read_link(&entry.path).map_err(|error| path_error(&entry.path, &error))?;
        line.push_str(" -> ");
        line.push_str(&escape(target.as_os_str().as_bytes()));
    }
    line.push('\n');
    out.write_all(line.as_bytes()).map_err(write_error)
}

/// Escape untrusted bytes without lossy replacement or terminal control interpretation.
fn escape(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                escape_text(text, &mut result);
                break;
            }
            Err(error) => {
                if let Ok(text) = std::str::from_utf8(&remaining[..error.valid_up_to()]) {
                    escape_text(text, &mut result);
                }
                let bad = error
                    .error_len()
                    .unwrap_or(remaining.len() - error.valid_up_to());
                for byte in &remaining[error.valid_up_to()..error.valid_up_to() + bad] {
                    result.push_str(&format!("\\x{byte:02x}"));
                }
                remaining = &remaining[error.valid_up_to() + bad..];
            }
        }
    }
    result
}
fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            c if c.is_control()
                || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}') =>
            {
                if (c as u32) < 256 {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                } else {
                    out.push_str(&format!("\\u{{{:x}}}", c as u32));
                }
            }
            c => out.push(c),
        }
    }
}
fn timestamp(seconds: i64) -> Result<String, String> {
    let days = seconds.div_euclid(86_400);
    let clock = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_march + 2) / 5 + 1;
    let month = month_march + if month_march < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    if !(0..=9999).contains(&year) {
        return Err("timestamp is outside RFC 3339 year range".into());
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        clock / 3600,
        clock / 60 % 60,
        clock % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_and_hostile_bytes() {
        assert_eq!(timestamp(0).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(-1).unwrap(), "1969-12-31T23:59:59Z");
        assert_eq!(timestamp(951782400).unwrap(), "2000-02-29T00:00:00Z");
        assert_eq!(escape(b"a\x1b\n\xff\\"), "a\\x1b\\x0a\\xff\\\\");
        assert_eq!(escape("x\u{202e}y".as_bytes()), "x\\u{202e}y");
    }
    #[test]
    fn files_tree_links_and_read_only_output() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "terms-listing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::write(root.join("z"), b"unchanged").unwrap();
        fs::write(root.join("a"), b"x").unwrap();
        fs::write(root.join(".hidden"), b"private").unwrap();
        fs::write(root.join("bad\nname"), b"bytes").unwrap();
        fs::create_dir(root.join("dir")).unwrap();
        fs::write(root.join("dir/child"), b"child").unwrap();
        symlink("dir", root.join("link")).unwrap();
        let mut out = Vec::new();
        execute(&[root.as_os_str().to_owned()], &mut out, false).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("a\nbad\\x0aname\n"));
        assert!(text.contains("link -> dir\n"));
        assert!(!text.contains(".hidden"));
        let mut out = Vec::new();
        execute(
            &[
                "--tree".into(),
                "--level".into(),
                "1".into(),
                root.as_os_str().to_owned(),
            ],
            &mut out,
            false,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("child"));
        assert_eq!(text.matches("link -> dir").count(), 1);
        let mut out = Vec::new();
        execute(
            &[
                "--long".into(),
                "--header".into(),
                root.join("a").into_os_string(),
            ],
            &mut out,
            false,
        )
        .unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with("TYPE MODE UID GID BYTES MODIFIED NAME\n- ")
        );
        assert_eq!(fs::read(root.join("z")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 6);
    }
    #[test]
    fn writer_failure_stops_listing() {
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            execute(&["Cargo.toml".into()], &mut Broken, false).unwrap_err(),
            "cannot write listing output"
        );
    }
    #[test]
    fn invalid_combinations() {
        for args in [
            vec!["--header"],
            vec!["--level", "1"],
            vec!["--tree", "--level", "257"],
            vec!["--sort", "bad"],
        ] {
            assert!(
                execute(
                    &args.into_iter().map(OsString::from).collect::<Vec<_>>(),
                    &mut Vec::new(),
                    false
                )
                .is_err()
            );
        }
    }
}

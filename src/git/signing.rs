//! User/system-authorized Git signing programs; repository keys remain data.
use super::backend::Git;
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Signing {
    pub summary: String,
    /// Global `-c` pairs followed by the commit-local signing flag.
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}
#[derive(Debug)]
struct Setting {
    scope: String,
    origin: String,
    key: String,
    value: String,
}
fn parse(bytes: &[u8]) -> Result<Vec<Setting>, String> {
    let parts: Vec<_> = bytes.split(|b| *b == 0).collect();
    if parts.last() != Some(&b"".as_slice()) || (parts.len() - 1) % 3 != 0 {
        return Err("malformed signing configuration response".into());
    }
    let mut out = Vec::new();
    for fields in parts[..parts.len() - 1].chunks_exact(3) {
        let field =
            std::str::from_utf8(fields[2]).map_err(|_| "signing configuration must be UTF-8")?;
        let (key, value) = field.split_once('\n').unwrap_or((field, "true"));
        out.push(Setting {
            scope: String::from_utf8(fields[0].to_vec()).map_err(|_| "invalid config scope")?,
            origin: String::from_utf8(fields[1].to_vec()).map_err(|_| "invalid config origin")?,
            key: key.into(),
            value: value.into(),
        });
    }
    Ok(out)
}
fn effective<'a>(settings: &'a [Setting], key: &str) -> Option<&'a Setting> {
    settings
        .iter()
        .rev()
        .find(|x| x.key.eq_ignore_ascii_case(key))
}
fn boolean(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" | "" => Ok(false),
        _ => Err("invalid commit.gpgSign boolean".into()),
    }
}
fn safe_program(value: &str, search: &str) -> Result<PathBuf, String> {
    // Git itself may shell-interpret metacharacters in configured programs.
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(&b))
    {
        return Err(
            "signing program must be a literal executable path/name without shell syntax".into(),
        );
    }
    let supplied = Path::new(value);
    let candidates: Vec<PathBuf> = if supplied.is_absolute() {
        vec![supplied.into()]
    } else if supplied.components().count() == 1 {
        search
            .split(':')
            .map(Path::new)
            .filter(|p| p.is_absolute())
            .map(|p| p.join(value))
            .collect()
    } else {
        return Err("relative signing program paths are unsupported".into());
    };
    for path in candidates {
        let Ok(path) = fs::canonicalize(path) else {
            continue;
        };
        let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
            continue;
        }
        if meta.permissions().mode() & 0o022 != 0 {
            return Err("signing executable is group/world writable".into());
        }
        if !path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(b))
        {
            return Err("canonical signing path contains unsupported shell syntax".into());
        }
        let mut parent = path.parent();
        while let Some(p) = parent {
            let m = fs::metadata(p).map_err(|e| e.to_string())?;
            if m.permissions().mode() & 0o022 != 0 {
                return Err("signing executable has a group/world-writable ancestor".into());
            }
            parent = p.parent();
        }
        return Ok(path);
    }
    Err(format!("trusted signing executable unavailable: {value}"))
}
fn resolve(settings: &[Setting], search: &str) -> Result<Signing, String> {
    if !effective(settings, "commit.gpgsign")
        .map(|x| boolean(&x.value))
        .transpose()?
        .unwrap_or(false)
    {
        return Ok(Signing {
            summary: "Signing: disabled by effective Git configuration".into(),
            args: vec!["--no-gpg-sign".into()],
            env: Vec::new(),
        });
    }
    let format = effective(settings, "gpg.format")
        .map(|x| x.value.as_str())
        .unwrap_or("openpgp");
    let (program_key, default) = match format {
        "ssh" => ("gpg.ssh.program", "ssh-keygen"),
        "openpgp" => ("gpg.openpgp.program", "gpg"),
        _ => return Err("only SSH and OpenPGP signing are implemented".into()),
    };
    let key = effective(settings, "user.signingkey").ok_or(
        "signed commits require configured user.signingKey; default-key commands are disabled",
    )?;
    if key.value.is_empty()
        || key
            .value
            .bytes()
            .any(|b| b == 0 || b == b'\n' || b == b'\r')
    {
        return Err("invalid signing key reference".into());
    }
    for setting in settings.iter().filter(|s| {
        s.key.eq_ignore_ascii_case(program_key)
            || format == "openpgp" && s.key.eq_ignore_ascii_case("gpg.program")
    }) {
        if !matches!(setting.scope.as_str(), "system" | "global") {
            return Err("repository/worktree signing-program overrides are not trusted".into());
        }
    }
    let program = settings.iter().rev().find(|p| {
        p.key.eq_ignore_ascii_case(program_key)
            || (format == "openpgp" && p.key.eq_ignore_ascii_case("gpg.program"))
    });
    let path = safe_program(program.map(|p| p.value.as_str()).unwrap_or(default), search)?;
    let origin = program
        .map(|p| format!("{} {}", p.scope, p.origin))
        .unwrap_or_else(|| "system default executable search".into());
    let mut args: Vec<OsString> = vec![
        "-c".into(),
        format!("gpg.format={format}").into(),
        "-c".into(),
        format!("{program_key}={}", path.display()).into(),
        "-c".into(),
        "gpg.ssh.defaultKeyCommand=".into(),
        format!("--gpg-sign={}", key.value).into(),
    ];
    if format == "openpgp" {
        args.splice(
            0..0,
            [
                OsString::from("-c"),
                format!("gpg.program={}", path.display()).into(),
            ],
        );
    }
    let names: &[&str] = if format == "ssh" {
        &["SSH_AUTH_SOCK"]
    } else {
        &["GNUPGHOME", "GPG_TTY"]
    };
    let env = names
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|v| (OsString::from(name), v)))
        .collect();
    Ok(Signing {
        summary: format!(
            "Signing: {format}\nKey reference: {}\nKey origin: {} {}\nProgram: {}\nProgram origin: {origin}",
            key.value,
            key.scope,
            key.origin,
            path.display()
        ),
        args,
        env,
    })
}
/// Resolve real Git configuration as data, authorizing executable origins separately.
pub fn policy(git: &Git) -> Result<Signing, String> {
    // --list works even when none of the optional signing keys is configured.
    let args: Vec<OsString> = [
        "config",
        "--includes",
        "--null",
        "--show-origin",
        "--show-scope",
        "--list",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let bytes = git.raw(&args, b"")?;
    let settings = parse(&bytes)?;
    // Parent process PATH is user context; exclude all relative/empty entries.
    let search = std::env::var("PATH")
        .unwrap_or_else(|_| "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin".into());
    resolve(&settings, &search)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_and_origin_policy() {
        let s=parse(b"local\0file:.git/config\0commit.gpgsign\ntrue\0local\0file:.git/config\0gpg.format\nssh\0local\0file:.git/config\0user.signingkey\nkey.pub\0local\0file:.git/config\0gpg.ssh.program\n/usr/bin/ssh-keygen\0").unwrap();
        assert!(resolve(&s, "/usr/bin").unwrap_err().contains("not trusted"));
    }
    #[test]
    fn disabled_and_invalid() {
        assert_eq!(
            resolve(&[], "/usr/bin").unwrap().args,
            vec![OsString::from("--no-gpg-sign")]
        );
        assert!(safe_program("gpg;touch", "/usr/bin").is_err());
        assert!(parse(b"broken").is_err());
    }
    #[test]
    fn ssh_defaults() {
        let s=parse(b"local\0file:repo\0commit.gpgsign\ntrue\0local\0file:repo\0gpg.format\nssh\0local\0file:repo\0user.signingkey\nkey.pub\0").unwrap();
        let result = resolve(&s, "/usr/bin").unwrap();
        assert!(result.summary.contains("ssh-keygen"));
        assert!(result.args.iter().any(|a| a == "--gpg-sign=key.pub"));
    }
}

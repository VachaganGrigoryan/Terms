//! Bounded system-Git boundary. Preflight detects supported hazards; it is not a sandbox.
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    os::unix::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
const LIMIT: usize = 16 * 1024 * 1024;
pub struct Git {
    pub executable: PathBuf,
    pub root: PathBuf,
}
pub struct Identity {
    pub author_name: String,
    pub author_email: String,
    pub committer_name: String,
    pub committer_email: String,
    pub summary: String,
}
fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
fn trim(mut v: Vec<u8>) -> Vec<u8> {
    if v.last() == Some(&b'\n') {
        v.pop();
    }
    v
}
fn escaped(v: &[u8]) -> String {
    v.iter()
        .flat_map(|b| std::ascii::escape_default(*b))
        .map(char::from)
        .collect()
}
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
fn terminate(child: &mut std::process::Child) {
    // SAFETY: this child was started in its own process group. Negative ID targets
    // only that owned group, including helpers retaining inherited pipe handles.
    unsafe {
        kill(-(child.id() as i32), 9);
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn invoke(
    exe: &Path,
    root: &Path,
    a: &[OsString],
    input: &[u8],
    raw: bool,
    env: &[(OsString, OsString)],
) -> Result<Vec<u8>, String> {
    if !exe.is_absolute() {
        return Err("Git executable must be absolute".into());
    }
    let mut c = Command::new(exe);
    c.current_dir(root).env_clear().process_group(0);
    // HOME/XDG are inherited unchanged only to resolve the user's actual Git
    // configuration. Operational invocations suppress global/system files.
    for key in ["HOME", "XDG_CONFIG_HOME", "TMPDIR"] {
        if let Some(v) = std::env::var_os(key) {
            c.env(key, v);
        }
    }
    c.env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PROTOCOL_FROM_USER", "0");
    if !raw {
        c.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
    }
    for (k, v) in env {
        if !matches!(
            k.to_str(),
            Some(
                "GIT_AUTHOR_NAME"
                    | "GIT_AUTHOR_EMAIL"
                    | "GIT_COMMITTER_NAME"
                    | "GIT_COMMITTER_EMAIL"
                    | "SSH_AUTH_SOCK"
                    | "GNUPGHOME"
                    | "GPG_TTY"
            )
        ) {
            return Err("unsupported Git environment override".into());
        }
        c.env(k, v);
    }
    // The `--no-lazy-fetch` option only exists in Git 2.44+ and older Git rejects it, breaking every
    // command. GIT_NO_LAZY_FETCH=1 (above) is the same switch where supported and is ignored
    // elsewhere; protocol.allow=never below blocks any transport, so a lazy fetch fails closed.
    c.args(["--no-pager", "--literal-pathspecs"]);
    for setting in [
        "core.fsmonitor=false",
        "core.hooksPath=/dev/null",
        "maintenance.auto=false",
        "gc.auto=0",
        "submodule.recurse=false",
        "fetch.recurseSubmodules=false",
        "protocol.allow=never",
        "credential.interactive=false",
        "user.useConfigOnly=true",
    ] {
        c.args(["-c", setting]);
    }
    if !raw {
        c.args([
            "-c",
            "commit.gpgSign=false",
            "-c",
            "tag.gpgSign=false",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "core.pager=",
        ]);
    }
    c.args(a)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("cannot launch Git: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("missing Git stdout")?;
    let mut stderr = child.stderr.take().ok_or("missing Git stderr")?;
    let mut stdin = child.stdin.take().ok_or("missing Git stdin")?;
    let (tx, rx) = mpsc::channel();
    let out_tx = tx.clone();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let r = stdout
            .by_ref()
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut b)
            .map(|_| b)
            .map_err(|e| e.to_string());
        let _ = out_tx.send((0, r));
    });
    let err_tx = tx.clone();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let r = stderr
            .by_ref()
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut b)
            .map(|_| b)
            .map_err(|e| e.to_string());
        let _ = err_tx.send((1, r));
    });
    if input.len() > LIMIT {
        terminate(&mut child);
        return Err("Git input exceeds 16 MiB".into());
    }
    let input = input.to_vec();
    std::thread::spawn(move || {
        let r = stdin
            .write_all(&input)
            .map(|_| Vec::new())
            .map_err(|e| e.to_string());
        drop(stdin);
        let _ = tx.send((2, r));
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let (mut out, mut err, mut written) = (None, None, false);
    let mut status = None;
    loop {
        if Instant::now() >= deadline {
            terminate(&mut child);
            return Err("Git operation timed out".into());
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(s) => status = s,
                Err(e) => {
                    terminate(&mut child);
                    return Err(e.to_string());
                }
            }
        }
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok((id, result)) => {
                let b = match result {
                    Ok(b) => b,
                    Err(e) => {
                        terminate(&mut child);
                        return Err(format!("Git pipe failure: {e}"));
                    }
                };
                if b.len() > LIMIT {
                    terminate(&mut child);
                    return Err("Git output exceeds 16 MiB".into());
                }
                match id {
                    0 => out = Some(b),
                    1 => err = Some(b),
                    _ => written = true,
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {}
        }
        if let (Some(code), Some(out), Some(err), true) =
            (status, out.as_ref(), err.as_ref(), written)
        {
            if code.success() {
                return Ok(out.clone());
            }
            return Err(format!(
                "Git exited {}: {}",
                code.code()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "by signal".into()),
                escaped(err)
            ));
        }
    }
}
pub fn discover(executable: PathBuf, dir: PathBuf) -> Result<Git, String> {
    if !executable.is_absolute()
        || !fs::metadata(&executable)
            .map_err(|e| e.to_string())?
            .is_file()
    {
        return Err("Git executable must be an absolute regular file".into());
    }
    let dir = fs::canonicalize(dir).map_err(|e| e.to_string())?;
    let root = trim(invoke(
        &executable,
        &dir,
        &args(&["rev-parse", "--show-toplevel"]),
        b"",
        false,
        &[],
    )?);
    use std::os::unix::ffi::OsStringExt;
    let g = Git {
        executable,
        root: PathBuf::from(OsString::from_vec(root)),
    };
    if trim(g.run(&args(&["rev-parse", "--is-bare-repository"]), b"")?) != b"false" {
        return Err("bare repositories are unsupported".into());
    }
    Ok(g)
}
impl Git {
    /// Data-only commands preserve Git's real include/conditional identity rules.
    pub fn raw(&self, a: &[OsString], input: &[u8]) -> Result<Vec<u8>, String> {
        let name = a
            .first()
            .and_then(|v| v.to_str())
            .ok_or("missing Git command")?;
        if !matches!(name, "config" | "var") {
            return Err("raw Git is restricted to config/var reads".into());
        }
        if name == "var"
            && !(a.len() == 2
                && matches!(
                    a[1].to_str(),
                    Some("GIT_AUTHOR_IDENT" | "GIT_COMMITTER_IDENT")
                ))
        {
            return Err("unsupported Git variable".into());
        }
        if name == "config"
            && !a.iter().skip(1).any(|x| {
                matches!(
                    x.to_str(),
                    Some("--get" | "--get-all" | "--get-regexp" | "--list" | "get" | "list")
                )
            })
        {
            return Err("Git config mutation prohibited".into());
        }
        if name == "config"
            && a.iter().any(|x| {
                matches!(
                    x.to_str(),
                    Some(
                        "--add"
                            | "--unset"
                            | "--unset-all"
                            | "--replace-all"
                            | "--remove-section"
                            | "--rename-section"
                            | "--edit"
                            | "set"
                            | "unset"
                            | "edit"
                    )
                )
            })
        {
            return Err("Git config mutation prohibited".into());
        }
        invoke(&self.executable, &self.root, a, input, true, &[])
    }
    pub fn run(&self, a: &[OsString], input: &[u8]) -> Result<Vec<u8>, String> {
        self.run_with_env(a, input, &[])
    }
    pub fn run_with_env(
        &self,
        a: &[OsString],
        input: &[u8],
        env: &[(OsString, OsString)],
    ) -> Result<Vec<u8>, String> {
        let mut i = 0;
        while a.get(i).is_some_and(|v| v == "-c") {
            let setting = a
                .get(i + 1)
                .and_then(|v| v.to_str())
                .ok_or("missing Git setting")?;
            let key = setting.split('=').next().unwrap_or("");
            if !matches!(
                key,
                "gpg.format"
                    | "gpg.program"
                    | "gpg.openpgp.program"
                    | "gpg.ssh.program"
                    | "user.signingKey"
                    | "user.signingkey"
                    | "commit.gpgSign"
                    | "commit.gpgsign"
            ) {
                return Err("unsupported Git configuration override".into());
            }
            i += 2;
        }
        let cmd = a
            .get(i)
            .and_then(|v| v.to_str())
            .ok_or("missing Git operation")?;
        if !matches!(
            cmd,
            "rev-parse"
                | "status"
                | "diff"
                | "log"
                | "show"
                | "for-each-ref"
                | "stash"
                | "check-attr"
                | "ls-files"
                | "add"
                | "restore"
                | "rm"
                | "update-index"
                | "commit"
        ) {
            return Err("unsupported Git operation".into());
        }
        if cmd == "stash" && a.get(i + 1).is_none_or(|v| v != "list") {
            return Err("only stash listing is supported".into());
        }
        let mut actual = a.to_vec();
        if matches!(cmd, "diff" | "log" | "show") {
            actual.splice(i + 1..i + 1, args(&["--no-ext-diff", "--no-textconv"]));
            if cmd != "diff" {
                actual.insert(i + 1, "--no-show-signature".into());
            }
        }
        if matches!(
            cmd,
            "status" | "diff" | "add" | "restore" | "rm" | "update-index" | "commit"
        ) {
            // These compare or stage worktree content, which runs repository-defined
            // clean/smudge commands. Blank every configured filter before Git starts.
            use std::os::unix::ffi::OsStringExt;
            let mut overrides = Vec::new();
            for name in self.filter_names()? {
                for setting in ["clean=", "smudge=", "process=", "required=false"] {
                    let mut key = b"filter.".to_vec();
                    key.extend(&name);
                    key.push(b'.');
                    key.extend(setting.as_bytes());
                    overrides.push(OsString::from("-c"));
                    overrides.push(OsString::from_vec(key));
                }
            }
            actual.splice(0..0, overrides);
        }
        invoke(&self.executable, &self.root, &actual, input, false, env)
    }
    /// Distinct `<name>` parts of every `filter.<name>.<var>` key in the effective config.
    fn filter_names(&self) -> Result<Vec<Vec<u8>>, String> {
        let keys = match self.raw(
            &args(&[
                "config",
                "--includes",
                "--null",
                "--name-only",
                "--get-regexp",
                "^filter\\.",
            ]),
            b"",
        ) {
            Ok(v) => v,
            Err(e) if e.starts_with("Git exited 1:") => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut names: Vec<Vec<u8>> = Vec::new();
        for key in keys.split(|b| *b == 0).filter(|k| !k.is_empty()) {
            let rest = &key[b"filter.".len()..];
            let Some(dot) = rest.iter().rposition(|b| *b == b'.') else {
                continue;
            };
            let name = &rest[..dot];
            // `-c name=value` splits at the first '=', so such a name cannot be blanked.
            if name.contains(&b'=') {
                return Err("unsupported Git filter name".into());
            }
            if !names.iter().any(|n| n == name) {
                names.push(name.to_vec());
            }
        }
        Ok(names)
    }
    fn config(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        match self.raw(&args(&["config", "--includes", "--get", key]), b"") {
            Ok(v) => Ok(Some(trim(v))),
            Err(e) if e.starts_with("Git exited 1:") => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn identity(&self) -> Result<Identity, String> {
        fn split(v: Vec<u8>) -> Result<(String, String), String> {
            let s = String::from_utf8(trim(v)).map_err(|_| "identity must be UTF-8")?;
            let l = s.rfind(" <").ok_or("invalid Git identity")?;
            let r = s[l + 2..].find('>').ok_or("invalid Git identity")? + l + 2;
            let name = s[..l].to_string();
            let email = s[l + 2..r].to_string();
            if name.is_empty()
                || email.is_empty()
                || name.contains(['\n', '\r', '\0'])
                || email.contains(['\n', '\r', '\0'])
            {
                return Err("invalid Git identity".into());
            }
            Ok((name, email))
        }
        let (an, ae) = split(self.raw(&args(&["var", "GIT_AUTHOR_IDENT"]), b"")?)?;
        let (cn, ce) = split(self.raw(&args(&["var", "GIT_COMMITTER_IDENT"]), b"")?)?;
        let origins = self.raw(
            &args(&[
                "config",
                "--includes",
                "--null",
                "--show-origin",
                "--show-scope",
                "--get-regexp",
                "^(user|author|committer)\\.(name|email)$",
            ]),
            b"",
        )?;
        let summary = format!(
            "Author: {an} <{ae}>\nCommitter: {cn} <{ce}>\nConfiguration origins: {}\n",
            escaped(&origins)
        );
        Ok(Identity {
            author_name: an,
            author_email: ae,
            committer_name: cn,
            committer_email: ce,
            summary,
        })
    }
    pub fn preflight(&self, write: bool, paths: &[OsString]) -> Result<(), String> {
        if !write {
            return Ok(());
        }
        for key in ["core.sparseCheckout", "core.sparseCheckoutCone"] {
            if self
                .config(key)?
                .is_some_and(|v| v.eq_ignore_ascii_case(b"true"))
            {
                return Err("writes in sparse checkouts are unsupported".into());
            }
        }
        for key in ["extensions.partialClone", "extensions.worktreeConfig"] {
            if key == "extensions.partialClone" && self.config(key)?.is_some() {
                return Err("writes in partial clones are unsupported".into());
            }
        }
        let ext = self.raw(
            &args(&[
                "config",
                "--includes",
                "--name-only",
                "--get-regexp",
                "^extensions\\.",
            ]),
            b"",
        );
        if let Ok(ext) = ext {
            for k in ext.split(|b| *b == b'\n') {
                if !k.is_empty()
                    && !matches!(
                        k,
                        b"extensions.objectformat"
                            | b"extensions.worktreeconfig"
                            | b"extensions.refstorage"
                    )
                {
                    return Err("unsupported repository extension".into());
                }
            }
        }
        let gitdir = self.run(&args(&["rev-parse", "--absolute-git-dir"]), b"")?;
        use std::os::unix::ffi::OsStringExt;
        let gitdir = PathBuf::from(OsString::from_vec(trim(gitdir)));
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "index.lock",
            "HEAD.lock",
        ] {
            if gitdir.join(marker).exists() {
                return Err(format!(
                    "repository busy or unsupported operation: {marker}"
                ));
            }
        }
        if !self
            .run(&args(&["ls-files", "--unmerged", "-z"]), b"")?
            .is_empty()
        {
            return Err("unmerged index is unsupported".into());
        }
        let mut input = Vec::new();
        for p in paths {
            if p.is_empty() {
                return Err("empty path operand".into());
            }
            let path = Path::new(p);
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err("paths must be worktree-relative without parent traversal".into());
            }
            if let Ok(m) = fs::symlink_metadata(self.root.join(path)) {
                if !(m.is_file() || m.file_type().is_symlink()) {
                    return Err("directory and special-file operands are unsupported".into());
                }
            }
            input.extend(p.as_bytes());
            input.push(0);
        }
        if !input.is_empty() {
            // Preserve effective global attributes for filter rejection, even though
            // operational Git suppresses global configuration. Neither read executes filters.
            let global = self
                .raw(
                    &args(&["config", "--includes", "--get", "core.attributesFile"]),
                    b"",
                )
                .ok()
                .map(trim);
            let mut check = args(&["check-attr", "--all", "-z", "--stdin"]);
            if let Some(v) = global {
                let mut setting = b"core.attributesFile=".to_vec();
                setting.extend(v);
                check.splice(0..0, ["-c".into(), OsString::from_vec(setting)]);
            }
            // check-attr is data-only; local attributes are retained, no helper runs.
            let attrs = invoke(&self.executable, &self.root, &check, &input, false, &[])?;
            let fields: Vec<_> = attrs.split(|b| *b == 0).collect();
            for group in fields.chunks(3) {
                if group.len() == 3
                    && group[1] == b"filter"
                    && !matches!(group[2], b"unspecified" | b"unset")
                {
                    return Err("staging filtered files (including LFS) is unsupported".into());
                }
            }
        }
        Ok(())
    }
    pub fn snapshot(&self, paths: &[OsString]) -> Result<Vec<u8>, String> {
        let mut out = self.run(
            &args(&[
                "status",
                "--porcelain=v2",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=all",
            ]),
            b"",
        )?;
        match self.run(&args(&["rev-parse", "--verify", "HEAD"]), b"") {
            Ok(v) => out.extend(v),
            Err(e) if e.starts_with("Git exited 128:") => out.extend(b"UNBORN"),
            Err(e) => return Err(e),
        }
        out.extend(self.raw(&args(&["config", "--includes", "--null", "--list"]), b"")?);
        let index = trim(self.run(&args(&["rev-parse", "--git-path", "index"]), b"")?);
        use std::os::unix::ffi::OsStringExt;
        let index = PathBuf::from(OsString::from_vec(index));
        let index = if index.is_absolute() {
            index
        } else {
            self.root.join(index)
        };
        if let Ok(f) = fs::File::open(index) {
            f.take((LIMIT + 1) as u64)
                .read_to_end(&mut out)
                .map_err(|e| e.to_string())?;
        }
        for p in paths {
            let path = self.root.join(p);
            match fs::symlink_metadata(&path) {
                Ok(m) if m.file_type().is_symlink() => out.extend(
                    fs::read_link(path)
                        .map_err(|e| e.to_string())?
                        .as_os_str()
                        .as_bytes(),
                ),
                Ok(m) if m.is_file() => {
                    let f = fs::File::open(path).map_err(|e| e.to_string())?;
                    f.take((LIMIT + 1) as u64)
                        .read_to_end(&mut out)
                        .map_err(|e| e.to_string())?;
                }
                Ok(_) => return Err("unsupported snapshot entry".into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => out.extend(b"MISSING"),
                Err(e) => return Err(e.to_string()),
            }
            if out.len() > LIMIT {
                return Err("repository snapshot exceeds 16 MiB".into());
            }
        }
        if out.len() > LIMIT {
            return Err("repository snapshot exceeds 16 MiB".into());
        }
        Ok(out)
    }
}

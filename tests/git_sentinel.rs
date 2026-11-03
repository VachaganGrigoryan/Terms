//! Phase 4 acceptance: repository-controlled helpers must never execute.
//!
//! Each fixture is a disposable repository whose config, hooks and attributes
//! point at a sentinel script. The script only appends its invocation to a
//! `hits` file outside the repository, so any entry there is an unexpected run.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

const GIT: &str = "/usr/bin/git";
static NEXT: AtomicUsize = AtomicUsize::new(0);
const HOOKS: [&str; 7] = [
    "pre-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "post-index-change",
    "reference-transaction",
    "pre-merge-commit",
];

struct Fixture {
    dir: PathBuf,
    repo: PathBuf,
    hits: PathBuf,
    sentinel: PathBuf,
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Fixture {
    /// A repo with one commit (`a.txt` = "one"), then `a.txt` modified and `b.txt` untracked.
    fn new() -> Option<Self> {
        if !Path::new(GIT).exists() {
            eprintln!("skipping: {GIT} is not available");
            return None;
        }
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("terms-sentinel-{}-{n}", std::process::id()));
        fs::create_dir_all(dir.join("home")).unwrap();
        let dir = fs::canonicalize(dir).unwrap();
        let repo = dir.join("repo");
        fs::create_dir(&repo).unwrap();
        let hits = dir.join("hits");
        let sentinel = dir.join("sentinel.sh");
        executable(
            &sentinel,
            &format!("#!/bin/sh\necho \"$0 $*\" >> '{}'\n", hits.display()),
        );
        fs::write(
            dir.join("home/.gitconfig"),
            "[user]\n\tname = Test User\n\temail = test@example.invalid\n",
        )
        .unwrap();
        let fx = Fixture {
            dir,
            repo,
            hits,
            sentinel,
        };
        fx.setup(&["init", "-q", "-b", "main"]);
        fs::write(fx.repo.join("a.txt"), "one\n").unwrap();
        fx.setup(&["add", "a.txt"]);
        fx.setup(&["commit", "-q", "-m", "base"]);
        fs::write(fx.repo.join("a.txt"), "two\n").unwrap();
        fs::write(fx.repo.join("b.txt"), "new\n").unwrap();
        Some(fx)
    }

    fn git(&self) -> Command {
        let mut c = Command::new(GIT);
        c.current_dir(&self.repo)
            .env_clear()
            .env("HOME", self.dir.join("home"))
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    /// Plain Git, only used before any hostile configuration is installed.
    fn setup(&self, args: &[&str]) {
        let o = self.git().args(args).output().unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    /// Read repository state with plumbing that cannot reach the sentinels.
    fn inspect(&self, args: &[&str]) -> String {
        let o = self
            .git()
            .args(["--no-pager", "-c", "core.fsmonitor=false"])
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }

    fn terms(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_terms"))
            .args(["git", "-C"])
            .arg(&self.repo)
            .args(args)
            .env_clear()
            .env("HOME", self.dir.join("home"))
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn terms_ok(&self, args: &[&str]) -> String {
        let o = self.terms(args);
        assert!(
            o.status.success(),
            "terms git {args:?} exited {:?}: {}",
            o.status.code(),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    fn assert_no_helpers_ran(&self) {
        let hits = fs::read_to_string(&self.hits).unwrap_or_default();
        assert!(hits.is_empty(), "repository helpers executed:\n{hits}");
    }

    fn append_config(&self, text: &str) {
        let path = self.repo.join(".git/config");
        let mut config = fs::read_to_string(&path).unwrap();
        config.push_str(text);
        fs::write(path, config).unwrap();
    }

    fn install_hooks(&self, dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        for name in HOOKS {
            let script = format!(
                "#!/bin/sh\nexec '{}' hook {name} \"$@\"\n",
                self.sentinel.display()
            );
            executable(&dir.join(name), &script);
        }
    }

    fn install_filter(&self) {
        let s = self.sentinel.display();
        self.append_config(&format!(
            "[filter \"sentinel\"]\n\tclean = {s}\n\tsmudge = {s}\n\tprocess = {s}\n\trequired = true\n"
        ));
        fs::write(self.repo.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn hostile_config_never_executes_helpers() {
    let Some(fx) = Fixture::new() else { return };
    let s = fx.sentinel.display().to_string();
    fx.install_hooks(&fx.repo.join(".git/hooks"));
    let alternate = fx.dir.join("hookspath");
    fx.install_hooks(&alternate);
    fx.append_config(&format!(
        "[core]\n\tfsmonitor = {s}\n\tpager = {s}\n\teditor = {s}\n\thooksPath = {}\n\
         [diff]\n\texternal = {s}\n\
         [diff \"sentinel\"]\n\tcommand = {s}\n\ttextconv = {s}\n\
         [pager]\n\tdiff = {s}\n\tlog = {s}\n\tshow = {s}\n\tstatus = {s}\n\tbranch = {s}\n",
        alternate.display()
    ));
    fs::write(fx.repo.join(".gitattributes"), "*.txt diff=sentinel\n").unwrap();

    assert!(fx.terms_ok(&["status"]).contains("a.txt"));
    assert!(fx.terms_ok(&["diff"]).contains("+two"));
    assert!(fx.terms_ok(&["log", "--limit", "5"]).contains("base"));
    assert!(!fx.terms_ok(&["show", "HEAD"]).is_empty());
    fx.terms_ok(&["branches"]);
    assert_eq!(fx.terms_ok(&["stashes"]), "");
    fx.terms_ok(&["identity"]);
    assert!(
        fx.terms_ok(&["stage", "a.txt"])
            .starts_with("Preview stage")
    );
    fx.terms_ok(&["stage", "--apply", "a.txt"]);
    fx.terms_ok(&["unstage", "--apply", "a.txt"]);
    fx.terms_ok(&["stage", "--apply", "a.txt"]);
    let preview = fx.terms_ok(&["commit", "-m", "second"]);
    assert!(preview.starts_with("Preview commit"));
    fx.terms_ok(&["commit", "--apply", "-m", "second"]);
    fx.assert_no_helpers_ran();

    assert_eq!(fx.inspect(&["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(fx.inspect(&["cat-file", "-p", "HEAD:a.txt"]), "two\n");
}

#[test]
fn filtered_files_are_refused_for_staging() {
    let Some(fx) = Fixture::new() else { return };
    fx.install_filter();
    let before = fx.inspect(&["ls-files", "--stage"]);
    for args in [&["stage", "a.txt"][..], &["stage", "--apply", "a.txt"][..]] {
        let o = fx.terms(args);
        let stderr = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(stderr.contains("filtered files"), "{args:?}: {stderr}");
    }
    fx.assert_no_helpers_ran();
    assert_eq!(fx.inspect(&["ls-files", "--stage"]), before);
}

#[test]
fn read_operations_do_not_run_clean_filters() {
    let Some(fx) = Fixture::new() else { return };
    fx.install_filter();
    for op in ["status", "diff"] {
        fx.terms_ok(&[op]);
    }
    fx.assert_no_helpers_ran();
}

#[test]
fn stashes_lists_existing_entries() {
    let Some(fx) = Fixture::new() else { return };
    fx.setup(&["stash", "push", "-q", "-m", "wip"]);
    let listing = fx.terms_ok(&["stashes"]);
    assert!(
        listing.contains("stash@{0}") && listing.contains("wip"),
        "{listing}"
    );
}

#[test]
fn unblankable_filter_name_fails_closed() {
    let Some(fx) = Fixture::new() else { return };
    let s = fx.sentinel.display();
    // `git -c name=value` splits at the first '=', so this filter cannot be overridden.
    fx.append_config(&format!("[filter \"a=b\"]\n\tclean = {s}\n"));
    fs::write(fx.repo.join(".gitattributes"), "*.txt filter=a=b\n").unwrap();
    let o = fx.terms(&["status"]);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("unsupported Git filter name"), "{stderr}");
    fx.assert_no_helpers_ran();
}

#[test]
fn repository_signing_program_is_not_trusted() {
    let Some(fx) = Fixture::new() else { return };
    fx.terms_ok(&["stage", "--apply", "a.txt"]);
    let s = fx.sentinel.display();
    fx.append_config(&format!(
        "[commit]\n\tgpgSign = true\n[user]\n\tsigningKey = ABCDEF\n[gpg]\n\tprogram = {s}\n"
    ));
    let o = fx.terms(&["commit", "--apply", "-m", "must not be created"]);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("not trusted"), "{stderr}");
    fx.assert_no_helpers_ran();
    assert_eq!(fx.inspect(&["rev-list", "--count", "HEAD"]).trim(), "1");
}

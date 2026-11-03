//! `terms init` prints the shell adapters embedded in the binary.
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn terms(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_terms"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(args: &[&str]) -> String {
    let out = terms(args);
    assert_eq!(out.status.code(), Some(0), "{args:?}");
    assert!(out.stderr.is_empty(), "{args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn adapter(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("shell")
        .join(name);
    fs::read_to_string(path).unwrap()
}

#[test]
fn prints_adapter_then_enables_it() {
    for (shell, file) in [("zsh", "terms.zsh"), ("bash", "terms.bash")] {
        let out = stdout(&["init", shell]);
        let text = adapter(file);
        assert!(
            out.starts_with(&text),
            "{shell}: adapter not embedded verbatim"
        );
        assert_eq!(&out[text.len()..], "terms_enable\n", "{shell}");
    }
}

#[test]
fn track_flag_consents_to_recording() {
    for shell in ["zsh", "bash"] {
        let out = stdout(&["init", shell, "--track"]);
        assert!(out.ends_with("terms_enable --track\n"), "{shell}");
    }
}

#[test]
fn history_flag_defines_but_never_enables_capture() {
    for (shell, file) in [("zsh", "history.zsh"), ("bash", "history.bash")] {
        let out = stdout(&["init", shell, "--history"]);
        assert!(
            out.contains(&adapter(file)),
            "{shell}: history adapter missing"
        );
        assert!(
            !out.lines().any(|l| l.trim() == "terms_history_enable"),
            "{shell}: capture must stay an explicit user action"
        );
        assert!(out.ends_with("terms_enable\n"), "{shell}");
    }
}

#[test]
fn bad_usage_is_rejected_without_output() {
    let cases: [&[&str]; 5] = [
        &["init"],
        &["init", "fish"],
        &["init", "zsh", "--bogus"],
        &["init", "zsh", "bash"],
        &["init", "--track"],
    ];
    for args in cases {
        let out = terms(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(!out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn help_lists_init() {
    assert!(stdout(&["init", "--help"]).starts_with("init "));
    assert!(stdout(&["--help"]).contains("init"));
}

#[test]
fn evaluates_in_real_shells() {
    for shell in ["/bin/zsh", "/bin/bash"] {
        if !Path::new(shell).exists() {
            eprintln!("skipping: {shell} is not available");
            continue;
        }
        let name = shell.rsplit('/').next().unwrap();
        let script = format!("eval \"$(\"$TERMS_BIN\" init {name})\" && type tj tl tg >/dev/null");
        let out = Command::new(shell)
            .args(["-c", &script])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TERMS_BIN", env!("CARGO_BIN_EXE_terms"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{shell}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

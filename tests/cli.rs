use std::process::Command;

#[test]
fn public_cli_contract() {
    for flag in ["--help", "--version"] {
        let result = Command::new(env!("CARGO_BIN_EXE_terms"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains(if flag == "--help" {
            "Commands:"
        } else {
            env!("CARGO_PKG_VERSION")
        }));
    }
    for arg in ["jump", "pick", "history", "--unknown", "unknown"] {
        let result = Command::new(env!("CARGO_BIN_EXE_terms"))
            .arg(arg)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert!(!result.stderr.is_empty());
    }
    // Phase 4 contract: bare `terms git` prints usage and succeeds.
    let result = Command::new(env!("CARGO_BIN_EXE_terms"))
        .arg("git")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert!(result.stderr.is_empty());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .starts_with("Usage: terms git")
    );
}

#[test]
fn streaming_json_pipeline() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_terms"))
        .args(["json", "-c", ".[] | select(.fish==\"tuna\")"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"[{"fish":"cod"},{"fish":"tuna"}]"#)
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(result.stdout, b"{\"fish\":\"tuna\"}\n");
}

#[test]
fn filesystem_and_offline_help() {
    let root = std::env::temp_dir().join(format!("terms-phase1-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("sample.txt"), b"cod\ntuna\n").unwrap();
    std::fs::write(root.join(".hidden"), b"secret").unwrap();
    let listing = Command::new(env!("CARGO_BIN_EXE_terms"))
        .arg("ls")
        .arg(&root)
        .output()
        .unwrap();
    assert!(listing.status.success(), "{:?}", listing.stderr);
    assert!(listing.stdout.windows(10).any(|w| w == b"sample.txt"));
    assert!(!listing.stdout.windows(7).any(|w| w == b".hidden"));
    let search = Command::new(env!("CARGO_BIN_EXE_terms"))
        .args(["search", "-F", "tuna"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(search.status.success(), "{:?}", search.stderr);
    assert!(search.stdout.windows(4).any(|w| w == b"tuna"));
    let help = Command::new(env!("CARGO_BIN_EXE_terms"))
        .args(["help", "--list"])
        .output()
        .unwrap();
    assert!(help.status.success(), "{:?}", help.stderr);
    assert!(help.stdout.windows(4).any(|w| w == b"json"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn json_constructed_depth_is_bounded() {
    use std::io::Write;
    use std::process::Stdio;
    let filter = vec!["[.]"; 5000].join(" | ");
    let mut child = Command::new(env!("CARGO_BIN_EXE_terms"))
        .args(["json", &filter])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"0").unwrap();
    let result = child.wait_with_output().unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(!result.stderr.is_empty());
}

#[test]
fn directory_store_roundtrip() {
    let root = std::env::temp_dir().join(format!("terms-jump-process-{}", std::process::id()));
    std::fs::create_dir_all(root.join("work")).unwrap();
    let invoke = |args: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_terms"))
            .arg("jump")
            .args(args)
            .env("XDG_STATE_HOME", root.join("state"))
            .output()
            .unwrap()
    };
    let result = invoke(&[std::ffi::OsStr::new("list")]);
    assert!(matches!(result.status.code(), Some(0) | Some(1)));
    assert!(!root.join("state").exists());
    let path = root.join("work");
    let result = invoke(&[std::ffi::OsStr::new("add"), path.as_os_str()]);
    assert!(result.status.success(), "{:?}", result.stderr);
    let result = invoke(&[
        std::ffi::OsStr::new("query"),
        std::ffi::OsStr::new("--hex"),
        std::ffi::OsStr::new("work"),
    ]);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(
        result
            .stdout
            .iter()
            .all(|b| b.is_ascii_hexdigit() || *b == b'\n')
    );
    std::fs::remove_dir_all(root).unwrap();
}

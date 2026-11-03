# Terms — agent notes

Rust (edition 2024, MSRV 1.85) single binary `terms`: an independently implemented terminal toolkit for macOS and Linux. Subcommands: `ls`, `help`, `search`, `json`, `pick`, `jump`, `history`, `git`. Each is a documented *subset* of an upstream tool; do not promise drop-in compatibility. Do not copy upstream code or help text.

## Source of truth
- `docs/phase-N.md` contracts win over README and `docs/roadmap.md`. Both of those are partly stale: README says Git is not implemented, and the roadmap still says the workspace is empty. `src/git/` and `docs/phase-4.md` are implemented.
- Verification reports in `docs/phase-N-verification.md` record real evidence and open gaps.

## Layout
- `src/lib.rs`: `execute(args, input, out, terminal)` dispatcher and top-level usage. `src/main.rs`: thin wrapper; errors are escaped and exit 2.
- One module per command. History is in `src/history/` (`agent`, `vault`, `model`, `mod`). Git is in `src/git/` (`backend`, `signing`, `browser`, `mod`). Shared pieces are `native.rs` (FFI/syscalls) and `pick.rs` (selector).
- `shell/`: zsh and bash adapters. `terms.*` covers jump/pick; `history.*` covers capture.
- Tests: `tests/cli.rs` (Rust CLI contract), `tests/*.py` (PTY selector, FIFO store), `shell/tests/*.py` (real Bash/Zsh sessions).

## Build and gate
```sh
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
cargo build --release --locked --offline
python3 shell/tests/adapters.py && python3 shell/tests/history_adapters.py
python3 tests/pty_selector.py && python3 tests/store_fifo.py
```
Use `--locked --offline`. Do not add dependencies without the review process in the phase docs.

**Current state (checked 2026-11-04):** the Rust build is broken. `cargo build` fails with two E0034 "multiple applicable items" errors on `by_ref` in `src/git/backend.rs:561` and `:577`, plus an unused `OsStr` import at `src/git/backend.rs:3`. `cargo fmt --check` reports diffs in `src/git/browser.rs`, `src/git/mod.rs`, `src/git/signing.rs`, and `src/lib.rs`. Clippy has six errors in total (two are the E0034 above; the rest are unlocated `else if` lints). Until those are fixed, `cargo test` and clippy cannot give a result. The four Python harnesses pass.

## Hard constraints
- Std-only, apart from the pinned crypto crates `argon2`, `chacha20poly1305`, `getrandom`, and `zeroize`. They use exact `=` versions and must build offline.
- `unsafe` (about 30 sites) is confined to FFI and syscalls: `getuid`, `flock`, `poll`, `signal`, `getpeereid`, `listen`. Keep it there and document each new site.
- Human output escapes untrusted terminal bytes. NUL and hex modes are for exact transport.
- Never edit shell startup files. Passphrases come from the tty only, never from arguments or environment variables. No plaintext fallback for history.
- Git: fixed `/usr/bin/git` (or `--git-bin` absolute path), with no repo-controlled executables. Hooks, pagers, fsmonitor, textconv, external diff, and submodules are disabled. Writes preview by default and apply only with `--apply`, after a snapshot recheck.
- Visited-directory data (`jump`) is plaintext in owner-only files under `$XDG_STATE_HOME/terms`. History is encrypted separately.

## Exit codes
0 success, 1 no results, 2 usage, data, I/O, or resource failure, 130 interactive cancel.

## Open acceptance gaps
From the phase reports, not yet closed: live history agent socket (sandbox EPERM), Bash 4.4+ and Linux runs, independent review of the crypto and Git diffs. Do not claim them done.

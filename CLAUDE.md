# Terms — agent notes

Rust (edition 2024, MSRV 1.85) single binary `terms`: an independently implemented terminal toolkit for macOS and Linux. Subcommands: `ls`, `help`, `search`, `json`, `pick`, `jump`, `history`, `git`. Each is a documented *subset* of an upstream tool; do not promise drop-in compatibility. Do not copy upstream code or help text.

## Source of truth
- `docs/phase-N.md` contracts win over `docs/roadmap.md`, whose "Current baseline" paragraph (empty workspace) is stale.
- Verification reports in `docs/phase-N-verification.md` record real evidence and open gaps.

## Layout
- `src/lib.rs`: `execute(args, input, out, terminal)` dispatcher and top-level usage. `src/main.rs`: thin wrapper; errors are escaped and exit 2.
- One module per command. History is in `src/history/` (`agent`, `vault`, `model`, `mod`). Git is in `src/git/` (`backend`, `signing`, `browser`, `mod`). Shared pieces are `native.rs` (FFI/syscalls) and `pick.rs` (selector).
- `shell/`: zsh and bash adapters. `terms.*` covers jump/pick and the short names (`t tl ts tq tp th tg tj tji`); `history.*` covers capture. `src/init.rs` embeds all four with `include_str!` for `terms init zsh|bash [--track] [--history]`, so **editing `shell/*` changes the binary**: rebuild before testing.
- Tests: `tests/cli.rs` (Rust CLI contract), `tests/init.rs`, `tests/git_sentinel.rs`, `tests/*.py` (PTY selector, FIFO store), `shell/tests/*.py` (real Bash/Zsh sessions). The Python harnesses honor `TERMS_BIN`.
- Release and install: `install.sh`, `scripts/{test-install,test-formula,gen-formula,update-tap}.sh`, `.github/workflows/{ci,release}.yml`, `docs/releasing.md`. README GIFs: `docs/demo/*.tape` plus `docs/demo/record.sh` (VHS in Docker), output in `docs/img/`.

## Build and gate
```sh
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
cargo build --release --locked --offline
python3 shell/tests/adapters.py && python3 shell/tests/history_adapters.py
python3 tests/pty_selector.py && python3 tests/store_fifo.py
sh scripts/test-install.sh && sh scripts/test-formula.sh
```
Use `--locked --offline` locally; CI uses `--locked` online. Do not add dependencies without the review process in the phase docs. `pty_selector.py` needs a debug build and `store_fifo.py` a release build (or set `TERMS_BIN`).

CI (`.github/workflows/ci.yml`) runs this gate on macOS, Ubuntu latest and Ubuntu 22.04, plus a Rust 1.85 job. Docker is handy for local Linux runs: use a `rust:1` container with `CARGO_TARGET_DIR` outside the repo. The gate also passed in Debian 13 (aarch64), Ubuntu 22.04 (Git 2.34), the Rust 1.85 image (Git 2.39) and static musl builds. Releasing is `docs/releasing.md`: a `vX.Y.Z` tag runs the release workflow, and running it on a branch is a dry run.

The whole gate passed on 2026-11-04 (macOS arm64, Rust 1.99.1, Apple Git 2.54.0); see `docs/phase-4-verification.md`. `tests/git_sentinel.rs` builds hostile repositories and fails if any repository-controlled helper runs; it skips when `/usr/bin/git` is missing. Any new `terms git` operation that touches worktree content, config, or hooks needs a sentinel test before it ships.

## Hard constraints
- Std-only, apart from the pinned crypto crates `argon2`, `chacha20poly1305`, `getrandom`, and `zeroize`. They use exact `=` versions and must build offline.
- `unsafe` (about 30 sites) is confined to FFI and syscalls: `getuid`, `flock`, `poll`, `signal`, `getpeereid`, `listen`. Keep it there and document each new site.
- Human output escapes untrusted terminal bytes. NUL and hex modes are for exact transport.
- Never edit shell startup files. Passphrases come from the tty only, never from arguments or environment variables. No plaintext fallback for history.
- Git must keep working on Git 2.34 (Ubuntu 22.04): never pass options newer than that (`--no-lazy-fetch` once broke every command on Debian 12 and Ubuntu); CI's Ubuntu 22.04 job guards this.
- Git: fixed `/usr/bin/git` (or `--git-bin` absolute path), with no repo-controlled executables. Hooks, pagers, fsmonitor, textconv, external diff, and submodules are disabled. Writes preview by default and apply only with `--apply`, after a snapshot recheck.
- Visited-directory data (`jump`) is plaintext in owner-only files under `$XDG_STATE_HOME/terms`. History is encrypted separately.

## Exit codes
0 success, 1 no results, 2 usage, data, I/O, or resource failure, 130 interactive cancel.

## Open acceptance gaps
From the phase reports, not yet closed: history capture on Bash 4.4+ (the harness only asserts the Bash 3.2 refusal), real-terminal Readline/ZLE recall, independent review of the crypto and Git diffs. Linux is now exercised in CI. The history agent socket worked end to end in a real PTY on Linux while recording `docs/img/history.gif`, but that is an observation, not an automated test. Do not claim any of these done.

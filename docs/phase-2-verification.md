# Phase 2 verification

Implementation present; acceptance OPEN. Real-terminal runtime, Linux, minimum compiler, modern Bash, and immutable upstream-source gates have not passed. Original roadmap unchanged.

Implemented pick/jump, native macOS/Linux terminal boundary, transactional private plaintext directory store, and explicit Bash/Zsh adapters. No crypto dependencies, shell startup edits, automatic activation, or command history collection. New public test boundary: InteractiveRuntime and execute_pick_with_runtime. Production runtime polls native stdin; fake runtimes validate event scheduling without a tty.

## Final checks

macOS workstation, installed Rust/Cargo 1.99.1:

| Exact command | Exit | Evidence |
|---|---|---|
| `cargo fmt --check` | 0 | Passed |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 0 | Passed |
| `cargo test --locked --offline` | 0 | 27 unit + 5 process tests passed |
| `cargo build --release --locked --offline` | 0 | target/release/terms |
| `bash -n shell/terms.bash` | 0 | Syntax passed |
| `zsh -n shell/terms.zsh` | 0 | Syntax passed |
| `python3 shell/tests/adapters.py` | 0 | Bash3.2/Zsh encoding, hostile paths, cancellation and enumeration-failure fixtures passed |
| `python3 tests/store_fifo.py` | 0 | Database/lock FIFOs rejected promptly |
| `python3 tests/pty_selector.py` | 1 | BLOCKED: /dev/tty open EPERM, child exits2; no real-terminal pass |

The PTY harness exercises real acceptance, stalled-producer cancellation and termios restoration when host permissions permit it. Current failure is recorded rather than skipped.

Independent read-only review inspected native ABIs, store operations, selector scheduling and shell transport. Review found FIFO open blocking, stale batched-key selection, occupied binding detection and inherited-pipefail early acceptance issues. Fixes: nonblocking nofollow file opens; deferred dependent key actions with current-query matching; capability-gated explicit binding replacement; validated enumeration followed by local pipefail-disabled transport. Reviewer reran `cargo test --locked --offline pick::` (exit0, six tests) and reported no remaining findings in those incremental fixes. Six selector tests include fake runtime acceptance/cancellation and resumable work quanta.

## Remaining acceptance limits

- /dev/tty is denied in this sandbox. Real foreground rendering, resize, signals, suspend/resume, cleanup and Readline/ZLE binding behavior remain unverified.
- Linux and Rust1.85 are untested; only installed Bash3.2 and Zsh fixture tests ran, not modern Bash.
- Pinned zoxide/fzf reference retrieval failed (GitHub API network errors; web immutable-reference cache misses). Source review gate remains open; naming the tools is not evidence of an audit. Phase1 research gates remain open too.
- Store tests cover roundtrip/ranking, concurrent writers, exclusions, corruption, symlink rejection and FIFO rejection. Lock-timeout/crash-injection coverage remains incomplete. Directory ancestry uses path-based APIs; this is not a race-resistant dirfd sandbox against ancestor replacement.
- Bash3.2 cannot inspect bind-x ownership. Ordinary binding activation refuses; explicit --replace permits an unrecoverable replacement and disable leaves a guarded dormant trampoline. See phase-2.md.
- No claim of security superiority, upstream compatibility or performance parity follows from passing tests.

## macOS terminal-readiness fix

User reproduced terminal `POLLNVAL` (32) on real Zsh. macOS readiness now uses select rather than poll for /dev/tty; Linux polling remains unchanged. fd_set/timeval layouts were checked against the installed Darwin SDK; descriptor bounds and EINTR are handled. A socket-pair readiness/EOF test exercises the real select syscall without requiring a controlling tty. Initial select$1050 linkage failed on arm64; corrected to exported select, and the final build succeeded.

Final commands after correction: `cargo fmt --check`, `cargo test --locked --offline` (29 unit + 5 process tests), `cargo clippy --locked --offline --all-targets -- -D warnings`, `cargo build --release --locked --offline` all exit0. Rebuilt target/release/terms. Full interactive tty acceptance remains pending because sandbox tty access is denied.

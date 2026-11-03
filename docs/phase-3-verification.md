# Phase 3 verification

Implementation present; acceptance OPEN. Roadmap unchanged. Crypto dependencies are the explicitly authorized exception to std-only Rust, pinned in Cargo.toml/Cargo.lock. docs/history-dependencies.md inventories checksums/licenses/build scripts; this is not a completed dependency/advisory audit.

## Final local checks

macOS, installed Rust/Cargo1.99.1:

| Exact command | Exit | Result |
|---|---|---|
| `cargo fmt --check` | 0 | Formatting passed |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 0 | Passed |
| `cargo test --locked --offline` | 0 | 35 unit +5 process tests passed |
| `cargo build --release --locked --offline` | 0 | target/release/terms rebuilt |
| `bash -n shell/history.bash` | 0 | Syntax passed |
| `zsh -n shell/history.zsh` | 0 | Syntax passed |
| `python3 shell/tests/history_adapters.py` | 0 | Actual interactive Zsh fixture: capture/multiline/status/leading-space exclusion/no native replay; Bash3.2 refusal |
| `cargo test --offline history:: -- --test-threads=1` (independent reviewer) | 0 | Six history tests passed |

History tests cover authenticated vault roundtrip, wrong key/tamper/rewrite/trailing repair, model events, peer credentials/frame transport, durable capture/finish/exclusions/lock, absolute trickle deadline and repair after restart. No plaintext persistence fallback or automatic history migration.

Release-agent subprocess smoke attempted private temporary XDG_STATE_HOME, `history agent`, followed by status/lock. Harness exited1: agent returned Operation not permitted(os error1) before a live socket could be exercised. Do not count this as a passing daemon test. Socket-pair and state-machine tests are separate passing evidence.

Independent crypto/unsafe review found repair reachability, import routing, Argon2 scratch cleanup, sensitive temporary cleanup, slow-client lock delay and response timeout defects. These were corrected. Last minor hex temporary allocation was replaced with direct preallocated encoding after review; the full checks above were repeated. Native input stack buffer and passphrase batches now zeroize. No remaining demonstrated framing/repair/liveness/ownership/FFI defect was reported. Independent shell review found no confirmed defect and reran shell fixtures successfully. Excluded begin responses are handled separately from capture errors.

## Open acceptance gates and limits

- Full cryptographic dependency/advisory/source audit, independent known-answer vectors, crash injection and randomness-failure injection remain incomplete. An inventory is not an audit.
- Linux, declared Rust1.85, modern Bash4.4+, real terminal passphrase lifecycle and Readline/ZLE recall remain unverified. Bash3.2 automatic capture deliberately refuses.
- Actual background-agent socket integration is blocked by sandbox EPERM. Keyed operations pass in state tests, not an end-to-end unlocked daemon session. Update 2026-11-04: on Linux (Debian 13, aarch64, zsh 5.9, in Docker) a real PTY session recorded for `docs/img/history.gif` completed `history init`, `history unlock` (agent started), `terms_history_enable`, then `history search` returning the captured commands. That is a manual observation from a recording, not an automated test, and the macOS agent and modern Bash remain unobserved. The gate stays open.
- Passphrase entry reuses the signal-aware raw-terminal alternate-screen guard rather than a dedicated echo-only prompt. It does not echo the passphrase; prompt UX needs real-terminal validation.
- Import commits valid lines individually; partial imports on later errors are documented. Export requires explicit plaintext target and creates a new private file.
- Agent serializes one client at a time (below the planned maximum); absolute frame deadlines bound client stalls. Queue/load stress validation is incomplete.
- No secure erasure of snapshots/backups, no rollback proof, no protection against root/same-user unlocked compromise, swap/core dumps or arbitrary allocator/compiler copies. Directory ancestry is not a dirfd race-resistant sandbox.
- Native history-file persistence stays disabled after capture disable until shell exit. Existing plaintext history is untouched. Explicit manual history writes or user restoration of HISTFILE remain outside this guarantee.
- Prior phase acceptance gaps remain separately tracked. Nothing was installed or enabled in the user's shell; no system service was created.

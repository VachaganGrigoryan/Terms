# Phase 4 verification

Implementation present; acceptance OPEN. Roadmap unchanged. Phase 4 adds no dependencies. Earlier phases' open gates remain open; see their reports.

## Final local checks

macOS arm64, Rust/Cargo 1.99.1, `/usr/bin/git` 2.54.0 (Apple Git-157):

| Exact command | Exit | Result |
|---|---|---|
| `cargo fmt --check` | 0 | Formatting passed |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 0 | Passed |
| `cargo test --locked --offline` | 0 | 38 unit, 5 CLI/process, 6 Git sentinel tests passed |
| `cargo build --release --locked --offline` | 0 | target/release/terms rebuilt |
| `python3 shell/tests/adapters.py` | 0 | Bash and Zsh adapters passed |
| `python3 shell/tests/history_adapters.py` | 0 | Zsh capture passed; Bash 3.2 refusal passed |
| `python3 tests/pty_selector.py` | 0 | PTY selector passed |
| `python3 tests/store_fifo.py` | 0 | FIFO rejection passed |

## Defects found and fixed while closing the phase

- **Build was broken.** Ambiguous `by_ref` (`Read` and `Write` both in scope) in `src/git/backend.rs` snapshot code, an unused import, and unformatted code in `src/git` and `src/lib.rs`.
- **Repository clean filters ran on `status` and `diff`.** The first sentinel run showed a repository-defined `filter.<name>.clean` command executing during content comparison, contradicting the contract. Reproduced with plain Git outside Terms. Fixed in `Git::run_with_env`: for `status`, `diff`, `add`, `restore`, `rm`, `update-index` and `commit`, every configured `filter.<name>` has `clean`, `smudge` and `process` blanked and `required` set false. A filter name containing `=` cannot be expressed with `git -c` and is refused with `unsupported Git filter name`.
- **`terms git stashes` always failed** with `unsupported Git operation`: it ran `git reflog show`, which the backend allowlist rejects. It now runs `git stash list`.
- **`tests/cli.rs`** expected bare `terms git` to exit 2; the contract is usage and exit 0.

## Sentinel coverage (`tests/git_sentinel.rs`)

Each test builds a disposable repository whose config, hooks or attributes point at a script that records any invocation. Any record fails the test.

| Mechanism | Test | Expectation |
|---|---|---|
| Hooks (`.git/hooks` and `core.hooksPath`), fsmonitor, pager, editor, external diff, textconv | `hostile_config_never_executes_helpers` | Full inspect, stage, unstage and commit sequence succeeds; zero executions; commit and file contents verified with plumbing |
| Clean/smudge/process filters on read operations | `read_operations_do_not_run_clean_filters` | `status` and `diff` succeed; zero executions |
| Filtered files on write operations | `filtered_files_are_refused_for_staging` | Preview and apply refused; index unchanged; zero executions |
| Filter name that cannot be blanked | `unblankable_filter_name_fails_closed` | `status` refused; zero executions |
| Repository-supplied signing program | `repository_signing_program_is_not_trusted` | Commit refused; no commit created; zero executions |
| Stash listing | `stashes_lists_existing_entries` | Existing stash listed |

## Open acceptance gates and limits

- Linux, declared Rust 1.85 and Git versions other than Apple Git 2.54.0 are untested. Sentinel tests skip when `/usr/bin/git` is absent.
- `terms git browse` has no automated test and no real-terminal run.
- No real SSH or OpenPGP signed commit was created. Signing is covered by policy unit tests and the local-signer refusal test only.
- No independent review of `src/git/` has been done. Phases 1–3 each had one; this gate is open.
- Not exercised: submodules, credential helpers and network settings (disabled by flags, untested), `core.sshCommand`, filters supplied through `include.path`, unborn-HEAD unstage, sparse and partial-clone refusal.
- Preflight and snapshot checks are not a sandbox against a concurrent malicious writer. The filter list is read just before each command and can be outpaced by such a writer.
- Nothing was installed or enabled in the user's shell.

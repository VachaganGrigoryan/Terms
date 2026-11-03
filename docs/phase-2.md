# Phase 2: interactive selection and directory navigation

The original roadmap remains unchanged. Phase 1's outstanding immutable-source, Linux and minimum-compiler verification gates remain open. This contract describes Phase 2, not proof that every gate has passed; see the phase verification report for actual results.

## Selector and runtime

`terms pick [--exact] [--multi] [--read0] [--print0] [--query TEXT] [--hex]` reads LF or NUL records, preserves bytes and duplicate input IDs, and accepts a final unterminated record. UI goes to a foreground `/dev/tty`; stdout receives records only after acceptance/restoration. Enter accepts current results without waiting for producer EOF. Multi-selections retain IDs when matching changes and emit in input order; absent explicit selections, Enter accepts focus. Exact means substring; fuzzy means byte subsequence. ASCII smart case folds unless query contains uppercase ASCII. Fuzzy rank is gaps, first position, record length, input ID; exact omits gaps. Rendering escapes non-ASCII and hostile bytes, independently of original records.

Limits: 16 MiB record, 64 MiB candidate bytes, 100,000 records, 4 KiB query; shell-selected bytes 1 MiB before hex encoding. Hex conflicts with print0. Return 0 accepted, 1 no result, 130 cancelled, 2 error. Cancellation/error emits no records. Keys include arrows/Ctrl-N/P, Backspace, Ctrl-U, Tab, Enter, Escape and Ctrl-C. No previews, execution actions or server are supported.

An injectable event boundary handles candidate readiness, terminal input, resize and signals without a blocked reader-thread join. Native macOS/Linux ABI code must be small, safety-documented and reviewed. Save/restore terminal state on every ordinary exit/error and handled termination; restore before suspend and reinitialize on resume. No cleanup guarantee is possible after SIGKILL.

## Directory data

`terms jump` supports add/remove/clear/list/query and exclude add/remove/list. Query supports interactive, null or hex transport and optional terms. Canonical physical absolute paths are recorded; explicit directory symlinks resolve intentionally. Saturating visit counts and last timestamps rank by visits/(1+whole days since last visit), newest visit, shorter path, byte order. Clock rollback clamps age to zero. All query terms must match substrings using ASCII smart case; empty query considers all. Missing paths are filtered without rewriting the store.

This is plaintext activity data. Explicit tracking consent is required. Exclusions cover a path and descendants, prevent writes and prune matching existing records. Store at XDG_STATE_HOME/terms or HOME/.local/state/terms with owner validation, directory0700/files0600, symlink-resistant opens, advisory lock with two-second timeout, versioned bounded length records, same-directory exclusive temp write/sync/rename/directory sync. Reject corruption instead of resetting. Maximum 64 MiB, 10,000 paths, 1,000 exclusions and 16 KiB path. Read-only queries do not create storage. Eviction removes lowest-ranked records deterministically.

## Shell activation

Source `shell/terms.bash` or `shell/terms.zsh` explicitly. Set `TERMS_BIN` to the absolute trusted executable if it is not discoverable. Sourcing only defines functions; it does not activate tracking or edit startup files.

- `eval "$(terms init zsh|bash [--track])"` prints the adapter embedded in the binary followed by `terms_enable` (or `terms_enable --track`); sourcing `shell/terms.{zsh,bash}` and calling `terms_enable` remains equivalent. `terms init` only prints text and edits no file.
- `terms_enable` registers `tj`, `tji` and completion. It also defines short names `t`, `tl`, `ts`, `tq`, `tp`, `th` and `tg` for `terms` and its `ls`, `search`, `json`, `pick`, `history` and `git` commands; an occupied name is skipped with a warning, and `terms_disable` removes only what Terms installed. `terms_enable --track` additionally consents to visited-path recording; records initial and changed directories.
- `tj` uses an existing directory directly or a ranked query; no arguments goes home. `tji` always selects interactively.
- `terms_bindings_enable` separately requests Alt-C directory change and, where supported, Ctrl-T editable file insertion. Occupied keys are refused. Users may explicitly unbind chosen keys first; Terms does not erase existing bindings automatically.
- `terms_disable` disables tracking/functions/completion and removes bindings only where ownership can be checked. Startup files remain untouched.

Hex records are captured with complete producer exit status before builtin decoding. Reject malformed encoding and NUL. Preserve embedded/trailing newlines; never evaluate selections. Insertion individually quotes records into the existing editable buffer and does not execute them. File enumeration completes under pipefail before selection, using bounded hex capture (1 MiB of decoded filenames). Only then are validated NUL records fed to the selector, so deliberate early acceptance does not misclassify upstream SIGPIPE as a search failure. Enumeration failure prevents opening the picker.

Bash 3.2 refuses ordinary binding activation because existing shell bindings cannot be inspected reliably. `terms_bindings_enable --replace` explicitly permits replacing Alt-C and installs a guarded directory-change trampoline; the replaced binding cannot be automatically restored. Its `bind` lacks `-X`, so an installed directory binding cannot be reliably inspected on disable; leave it dormant rather than erase a potentially replaced user binding. This is an explicit lifecycle acceptance gap. Modern Bash supports insertion only when the tested Readline-buffer capability is available. Zsh uses ZLE widgets. Bash array PROMPT_COMMAND tracking is rejected instead of flattening user hooks; completion uses the user's initialized Zsh completion system. These limits require verification before broader shell-support claims.

## Validation gates

Run Cargo fmt, offline strict Clippy, tests and release build; real PTYs must test stalled producer cancellation, resize, suspend/resume, restoration and failures. Store tests must include concurrent writes, lock timeout, interruption, corruption, exclusions, ownership and symlink attacks. Shell tests cover quoting, hostile names, existing hooks, idempotence, cancellation and unchanged buffers. Run on macOS/Linux and Bash3.2/modern Bash/Zsh. Missing environments are gaps, not passes.

`python3 shell/tests/adapters.py` tests installed Bash and Zsh with an inert fixture executable: malformed/NUL encoding rejection, embedded/trailing newlines, shell-substitution text, cancellation, unchanged directory, failed enumeration preventing picker execution, short names working, an occupied name being skipped with a warning, and `terms_disable` removing only Terms' own definitions. It does not test a real selector, Readline/ZLE, real PTYs, modern Bash, or all existing-hook configurations. Python is test infrastructure only; the product remains std-only Rust/static shell adapters.

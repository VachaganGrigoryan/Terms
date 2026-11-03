# Terms

An independently implemented terminal toolkit for macOS and Linux: one small binary with `ls`, `search`, `json`, `pick`, `jump`, an encrypted `history`, a hardened `git`, and offline `help`. Terms supports documented subsets of the original tools, rather than drop-in compatibility.

![Enable the short names with eval "$(terms init zsh)"](docs/img/init.gif)

## Install

Prebuilt binaries cover macOS and Linux on Apple Silicon/ARM64 and x86_64. The Linux builds are static, so they run on any distribution, including Alpine.

**Homebrew** (macOS and Linux)

```sh
brew install VachaganGrigoryan/tap/terms
```

**Install script**

```sh
curl -fsSL https://raw.githubusercontent.com/VachaganGrigoryan/Terms/main/install.sh | sh
```

The script puts the `terms` binary in `~/.local/bin` (`TERMS_INSTALL_DIR` changes that, `TERMS_VERSION=v0.1.0` pins a version) after checking the download against the release's `SHA256SUMS`. It writes nothing else and never edits your shell startup files. The checksum catches a corrupted or altered download, not a compromised release, because both come from the same place. If you would rather read it first: `curl -fsSLO https://raw.githubusercontent.com/VachaganGrigoryan/Terms/main/install.sh`, then `sh install.sh`.

**Cargo**

```sh
cargo install --git https://github.com/VachaganGrigoryan/Terms --locked
```

**From source**

```sh
git clone https://github.com/VachaganGrigoryan/Terms && cd Terms
cargo build --release --locked
./target/release/terms --version
```

Requirements: building from source needs Rust 1.85 or newer. `terms git` needs Git 2.34 or newer at `/usr/bin/git`, or pass `--git-bin /absolute/path/to/git`. It was tested with Git 2.34, 2.39, 2.47 and 2.54. If you download an archive in a browser on macOS, Gatekeeper may quarantine it; clear that with `xattr -d com.apple.quarantine terms`. The script and Homebrew are unaffected.

## Shell setup

Terms works without any shell integration. For short command names, directory jumping and optional history capture, add one line to `~/.zshrc` (or `~/.bashrc`):

```sh
eval "$(terms init zsh)"      # or: eval "$(terms init bash)"
```

`terms init` only prints the integration; Terms never edits your startup files. It defines these short names, so you can type less:

| Name | Runs | Example |
|---|---|---|
| `t` | `terms` | `t help ls` |
| `tl` | `terms ls` | `tl --long --header` |
| `ts` | `terms search` | `ts -F TODO src` |
| `tq` | `terms json` | `tq '.name' data.json` |
| `tp` | `terms pick` | `ts --files \| tp` |
| `th` | `terms history` | `th search cargo` |
| `tg` | `terms git` | `tg status` |
| `tj`, `tji` | jump and change directory | `tj project` |

A name that is already an alias, function, builtin or command is skipped with a message on stderr and never replaced. `terms_disable` removes only the definitions Terms installed. The names exist only in shells where you ran the line above.

Options: `terms init zsh --track` also records every directory you visit (see [jump](#jump--remember-and-find-directories)), and `terms init zsh --history` defines the history functions, but capture still starts only when you run `terms_history_enable` (see [history](#history--encrypted-command-history)). Bash 3.2, the macOS default, gets the short names and directory jumping; automatic history capture needs Bash 4.4 or newer, and key bindings need an explicit `--replace` there (see [jump](#jump--remember-and-find-directories)).

The sections below use the full `terms` name; every example works with the short names too.

## Uninstall

```sh
brew uninstall terms          # if you used Homebrew
rm ~/.local/bin/terms         # if you used the install script
```

Remove the `eval "$(terms init ...)"` line from your shell startup file. Terms keeps its data under `$XDG_STATE_HOME/terms` (default `~/.local/state/terms`): remembered directories in plaintext, and the encrypted history vault. Delete that directory only if you want them gone.

## Status

Early release (0.1.x). CI builds and tests on macOS and on Linux (Ubuntu latest and 22.04), and with Rust 1.85. Some acceptance gates are still open and listed in the verification reports: independent cryptographic and Git review, history capture on modern Bash, and real-terminal validation of Readline/ZLE recall. The encrypted history format has not been independently audited. See the [roadmap](docs/roadmap.md) and [releasing](docs/releasing.md).

## Using Terms

Use `terms COMMAND --help` for command syntax. Pass option values separately (`--sort size`); combined short flags such as `-ln` are not supported. Use `--` before operands starting with `-`, where documented.

## ls — list files

![terms ls](docs/img/ls.gif)

```sh
terms ls                              # Current directory, one entry per line
terms ls --all                         # Include hidden entries
terms ls --long --header               # Detailed metadata with column headers
terms ls --tree --level 2              # Tree, with operand at depth zero
terms ls --sort size --reverse         # Largest entries first
terms ls --sort modified --reverse     # Newest entries first
terms ls --color never /tmp            # Disable terminal styling
terms ls -- ./-unusual-name            # Operand beginning with a dash
```

Long output includes file type, octal permissions, numeric UID/GID, bytes, UTC timestamp, and name. Names are escaped for terminal safety; symlinks are displayed without following them. `--header` requires `--long`, and `--level` requires `--tree`. Grid layout, icons, and Git status are not implemented.

## help — original offline command examples

```sh
terms help --list
terms help ls
terms help search
terms help --search directory
terms help --platform macos find
terms help --plain json
```

Pages are bundled and work offline. They cover implemented Terms commands and a small set of system commands, not the complete tldr corpus. Platform-specific pages fall back to common pages. Examples are never executed. `terms --help` is CLI usage; `terms help` is the offline documentation command.

## search — search files and enumerate paths

![terms search](docs/img/search.gif)

```sh
terms search -F 'needle' .                  # Literal recursive search
terms search -i -n 'error|warning' src       # Regex, ASCII case folding, line numbers
terms search --context 2 'error' src        # Surrounding lines
terms search -l -F 'TODO' src               # Names of matching files
terms search -c -F 'TODO' src               # Matching-line counts
terms search --hidden -F 'needle' .         # Include hidden paths, still skip .git
terms search --no-ignore -F 'needle' .       # Disable ignore-file rules
terms search --glob '*.rs' -F 'TODO' src    # Restrict file matches
terms search --files src                   # Enumerate files, no pattern
terms search --files --null src            # Exact path bytes, NUL delimiters
```

Recursive traversal respects the implemented `.gitignore`/`.ignore` subset and skips symlinks and special files. Binary files containing NUL are excluded from content search. Explicit regular-file operands bypass hidden/ignore filtering. `--null` is available only with `-l` or `--files`.

Regex supports literals, ASCII classes, grouping, alternation, anchors, `.`, `*`, `+`, and `?`. Lookaround, backreferences, counted repetition, multiline matching, Unicode case folding, compressed files, and preprocessors are not supported. Quote patterns to prevent shell expansion. Search reads files, not stdin; `-` is a literal filename.

## json — validate, format, and filter JSON

![terms json](docs/img/json.gif)

```sh
printf '{"name":"Ada","active":true}\n' | terms json '.'
terms json -c '.' data.json                # Compact output
terms json -r '.name' data.json            # Raw string output
terms json '.items[]' data.json            # Iterate an array
terms json '.[] | select(.fish=="tuna")' orders.json
terms json '[.[] | select(.active==true)]' users.json
terms json 'map(.name)' users.json
terms json '{name: .name, count: .items | length}' data.json
```

Input comes from stdin when no files are supplied. Multiple whitespace-separated JSON values are supported. Numbers retain their original tokens without floating-point conversion. Duplicate object keys and malformed Unicode are rejected.

Supported filters include paths/indexes, iteration, pipes, `select` equality/inequality against scalar literals, array/object construction, `map`, and `length`. Arithmetic, modules, variables, updates, and the full jq language are not implemented. Raw strings preserve bytes to pipes/files, but terminal controls are escaped when displayed directly in a terminal. `-` is a literal filename, not a stdin alias.

## pick — select records interactively

![terms pick](docs/img/pick.gif)

```sh
printf 'apple\nbanana\norange\n' | terms pick
printf 'apple\nbanana\norange\n' | terms pick --multi
printf 'apple\npineapple\npear\n' | terms pick --exact --query apple
terms search --files --null | terms pick --read0 --print0
```

Supply candidates through a pipe. Bare `terms pick` does not enumerate files. The interface reads keys from `/dev/tty` and prints accepted records to stdout; it does not open or execute selections.

| Key | Action |
|---|---|
| Type | Filter candidates |
| Up/Down or Ctrl-P/Ctrl-N | Move focus |
| Tab | Toggle focused selection with `--multi` |
| Enter | Accept selections, or the focused item |
| Backspace / Ctrl-U | Delete a character / clear query |
| Escape / Ctrl-C | Cancel without output |

Default matching is fuzzy byte subsequence matching; `--exact` uses substring matching. ASCII uppercase in the query enables case-sensitive matching. `--read0` reads NUL-delimited records; `--print0` writes them. `--hex` instead emits hex-encoded records for shell adapters. A real foreground controlling terminal is required.

## jump — remember and find directories

![tj and tji](docs/img/jump.gif)

Manual recording and queries:

```sh
terms jump add "$PWD"
terms jump add /path/to/project
terms jump list
terms jump query project                  # Print the best remembered path
terms jump query --interactive            # Choose a remembered directory
terms jump query --null project           # NUL-terminated path
terms jump exclude add /path/to/private
terms jump exclude list
terms jump exclude remove /path/to/private
terms jump remove /path/to/project
```

The binary prints paths; shell functions below actually change the parent shell's directory. Ranking uses visit count and recency. Missing paths are filtered from results. Exclusions cover a directory and descendants, prevent recording, and remove matching stored entries. `terms jump clear` deletes all remembered directory entries.

Visited paths are **plaintext activity data**, stored in owner-only files under `$XDG_STATE_HOME/terms`, or `$HOME/.local/state/terms` when unset. This store is separate from encrypted command history.

### Change directories from your shell

With `eval "$(terms init zsh)"` in place (see [Shell setup](#shell-setup)), these functions change the parent shell's directory:

```sh
tj project                                # Best remembered match
tji                                       # Interactive remembered-directory selection
tj /tmp                                   # Direct directory change
tj                                        # Home directory
terms_disable                             # Disable navigation integration
```

Tracking is off by default. `eval "$(terms init zsh --track)"` (or `terms_enable --track` in a shell where the integration is already loaded) explicitly consents to recording the initial and every changed directory.

Optional `terms_bindings_enable` requests Alt-C for interactive directory changes and, where supported, Ctrl-T for editable file-path insertion. Occupied bindings are refused. On Bash 3.2 ordinary binding activation is refused because shell bindings cannot be inspected reliably; explicit `terms_bindings_enable --replace` can replace Alt-C, but cannot restore the previous binding automatically. Disable leaves its guarded handler dormant. Navigation functions do not require these bindings.

## history — encrypted command history

![terms history](docs/img/history.gif)

History implementation exists, but acceptance is still open. See [verification status](docs/phase-3-verification.md) for live-agent, real-terminal, modern Bash/Linux, and dependency-review gaps. Git and encrypted-history tools from other projects are not runtime dependencies.

### Initialize and unlock

Run in a real terminal:

```sh
terms history init                        # Create vault; enter and confirm passphrase
terms history unlock                      # Unlock; starts a local agent if needed
terms history status
```

Initialization does not enable capture or import existing history. Passphrases are entered without echo, never through arguments or environment variables. The vault uses Argon2id and XChaCha20-Poly1305 through pinned crypto libraries. No plaintext fallback, sync, output capture, or system service is provided.

### Enable shell capture explicitly

Load the history functions, then switch capture on yourself:

```sh
eval "$(terms init zsh --history)"         # or: terms init bash --history (Bash 4.4+ for capture)
terms_history_enable
```

Automatic capture supports Zsh and Bash 4.4+; Bash 3.2 activation refuses. Incompatible existing hooks may also cause activation to refuse rather than overwrite them.

Capture records command text, directory, start time, duration, and exit status. Leading-space commands and bookkeeping calls are excluded. Long-running or interrupted commands can remain incomplete. Locked/failed recording is reported without queuing plaintext for later replay.

**Native history-file saving stays disabled for the remainder of this shell session**, including after `terms_history_disable`, to avoid writing captured commands back to plaintext on exit. Existing plaintext history files are untouched. A fresh shell uses its normal settings.

### Search and recall

```sh
terms history search                      # Newest records first
terms history search git                   # Match command text
terms history search --cwd "$PWD" cargo
terms history search --exit 1              # Commands with exit status 1
terms history search --session SESSION_ID
terms history pick                        # Interactively select and print a command
terms history pick --hex                  # Encoded result for shell integration
```

Selection alone does not execute a command. Optional `terms_history_bindings_enable` requests Ctrl-R for editable recall and refuses an occupied binding. `terms_history_bindings_disable` removes an owned binding. Actual Readline/ZLE integration has incomplete validation.

```sh
terms_history_disable                     # Stop this shell's capture
terms history lock                        # Clear unlocked key and decrypted index
```

The agent also locks after the last registered shell dies or 15 minutes without relevant activity. Commands finishing after automatic locking may remain incomplete. `terms history agent` runs the service in the foreground for diagnosis.

### Retention and exclusions

```sh
terms history retention
terms history retention --days 30 --max-commands 5000
terms history exclude add prefix 'secret-tool '
terms history exclude add dir /path/to/private
terms history exclude list
terms history exclude remove prefix 'secret-tool '
terms history exclude remove dir /path/to/private
```

Defaults are 90 days or 10,000 commands, whichever limit is reached first. Prefixes are literal, not shell commands or regular expressions. Exclusion and retention configuration require unlock.

### Import, export, deletion, and repair

```sh
terms history export --plaintext history-export.jsonl
terms history import history-export.jsonl
terms history delete COMMAND_ID           # ID from history search
```

Export intentionally creates a **plaintext** owner-only file and refuses to overwrite an existing file. Import accepts only Terms JSONL, assigns fresh IDs, and applies exclusions/retention. Fields are strings: `command_hex`, `cwd_hex`, `time`, `duration`, and `exit`; empty duration/exit denote incomplete commands. Import commits line by line, so an error can leave earlier lines imported. Native Bash/Zsh history files are not accepted automatically.

`terms history clear --all` deletes every live history record. Deletion cannot erase snapshots or backups.

`terms history repair --truncate-incomplete-tail` explicitly authenticates the valid prefix and removes only an incomplete final frame. It does not bypass authentication failures or silently discard complete corrupted frames. Back up the encrypted vault before deliberate recovery operations. At-rest encryption does not protect an unlocked session from root or same-user compromise, or guarantee detection of whole-vault rollback.

## git — inspect, stage and commit with system Git

![terms git](docs/img/git.gif)

```sh
terms git status                          # Escaped status
terms git status --null                   # Raw NUL-delimited porcelain v2
terms git diff --staged -- src/lib.rs     # Diff, optionally staged or limited to files
terms git log --limit 20                  # 1 to 1000, default 50
terms git show COMMIT
terms git branches
terms git stashes
terms git identity                        # Effective author/committer and config origins
terms git browse                          # Read-only interactive browser
terms git stage a.txt                     # Preview only
terms git stage --apply a.txt
terms git unstage --apply a.txt
terms git commit -m 'message'             # Preview only
terms git commit --apply -m 'message'
terms git -C /path/to/repo status         # Operate on another repository
```

Terms runs `/usr/bin/git` (or the absolute path given to `--git-bin`) with a cleared environment. Hooks, pagers, editors, fsmonitor, external diff, textconv, repository-defined filters, submodule recursion, maintenance and network access are disabled. Output escapes terminal controls.

Stage, unstage and commit show a preview by default. `--apply` rechecks the repository first and refuses if it changed. They take explicit files only (no directories), and refuse sparse checkouts, partial clones, unmerged or in-progress states, and files with a Git filter (including LFS). Commit needs `-m` or `--message-file`, commits exactly the staged index, and exits 1 when nothing is staged. Signing follows your configured SSH or OpenPGP settings; signing programs are trusted only from user, global or system configuration, never from the repository. Checkout, remotes, stash writes, rebase, conflict resolution, amend and partial staging are not implemented. See the [Phase 4 contract](docs/phase-4.md).

## Exit statuses and verification

Generally, `0` means success, `1` means no results, `2` means usage/data/I/O/resource failure, and `130` means interactive cancellation. Commands may have emitted earlier valid output before a later error. Human-oriented output escapes untrusted terminal content; NUL/hex modes are intended for exact transport.

```sh
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
cargo build --release --locked --offline
```

Rust 1.85 is the declared minimum and is built and tested in CI. The platforms and tool versions that were actually exercised are recorded in the verification reports. Third-party dependencies are limited to the approved crypto crates and their required dependencies. Terms never installs anything on its own, edits startup files, talks to the network, or migrates history; the install script and Homebrew only place the binary.

See the [roadmap](docs/roadmap.md), [research](docs/research.md), and verification reports for [Phase 1](docs/phase-1-verification.md), [Phase 2](docs/phase-2-verification.md), [Phase 3](docs/phase-3-verification.md), and [Phase 4](docs/phase-4-verification.md).

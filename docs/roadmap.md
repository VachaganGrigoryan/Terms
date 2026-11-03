# Terms: an independently implemented terminal toolkit

## Summary

Build our own Rust/Bash alternatives to the eight tools in your screenshots. Deliver useful features in stages, with a sourced research inventory covering additional capabilities and their implementation cost.

Your selected constraints:

- macOS and Linux; Bash, including macOS Bash 3.2, and Zsh.
- Rust standard library only, except reviewed cryptography dependencies.
- Native OS APIs and system Git permitted.
- Original implementation and help examples.
- Encrypted history with a passphrase-unlocked local agent.
- Independent commands rather than promised drop-in compatibility.

The security objective is a smaller, inspectable trust boundary. A rewrite’s security must be demonstrated through review and testing.

## Feature inventory and intended replacements

Use one binary with eight subcommands, sharing traversal, terminal rendering, selection, and storage code.

| Reference | Our command | Useful capabilities to reproduce |
|---|---|---|
| [eza](https://github.com/eza-community/eza) | `terms ls` | Grid, detailed listing, tree, hidden files, sorting, permissions, sizes, timestamps, icons, symlinks, headers, Git status. Later: extended attributes, mount details, themes, directory totals. |
| [zoxide](https://github.com/ajeetdsouza/zoxide) | `terms jump` | Directory visit tracking, frequency/recency ranking, multi-term lookup, interactive selection, exclusions, removal, completion. |
| [fzf](https://github.com/junegunn/fzf) | `terms pick` | Fuzzy/exact filtering, case controls, multiple selection, field selection, keyboard navigation, NUL-delimited records. Omit executable previews and command bindings initially. |
| [ripgrep](https://github.com/BurntSushi/ripgrep) | `terms search` | Recursive search, literal/regex patterns, ignore rules, globs, case controls, context, counts, filenames, line numbers, structured output. Defer PCRE2, decompression, subprocess preprocessing, and legacy encodings. |
| [tldr](https://github.com/tldr-pages/tldr/blob/main/CLIENT-SPECIFICATION.md) | `terms help` | Offline examples, platform-specific pages, multiword commands, listing, keyword search, plain/formatted output. Author our own initial corpus. |
| [Atuin](https://docs.atuin.sh/latest/guide/basic-usage/) | `terms history` | Encrypted command history; directory, session, time, duration and exit-status filters; interactive recall; exclusions; retention; deletion; explicit import/export. No sync or output capture. |
| [lazygit](https://github.com/jesseduffield/lazygit) | `terms git` | Status, diffs, history, branches and stash inspection; later file staging, unstaging, and commits. Defer checkout, stash mutation, partial staging, rebase, conflict resolution, and remote operations. |
| [jq](https://jqlang.org/manual/) | `terms json` | Validation, formatting, field/index access, iteration, pipes, selection, projection, `map`, `length`, compact/raw output, and streams of JSON values. Defer modules, arbitrary functions, arithmetic, and advanced transformations. |

Create `docs/research.md` containing upstream source URLs and revisions, documented feature inventories, security-relevant execution paths, and a supported/deferred/excluded matrix. Use documentation and source inspection; do not install or execute upstream tools as part of the initial research.

## Implementation stages

**1. Foundations and noninteractive tools**

- Separate original Rust modules behind the `terms` CLI; Bash/Zsh provide small static adapters.
- Implement byte-preserving paths and records, escaped human output, NUL-safe machine transport, bounded parsers, and explicit errors for unsupported options.
- Deliver listing, offline help, literal search, and the JSON subset first. Include the screenshot’s filter: `.[] | select(.fish=="tuna")`.
- Preserve JSON number tokens without floating-point conversion. Support UTF-8 JSON, validated escapes and surrogate pairs; reject duplicate object keys and excessive nesting.
- Search skips hidden files, binary files and directory symlinks by default. Add an independently written Thompson-NFA regex engine supporting literals, classes, grouping, alternation, anchors and `* + ?`; reject lookaround, backreferences and counted repetition.
- Implement documented ignore rules for nested patterns, negation and directory exclusions; test precedence rather than claiming complete ripgrep compatibility.

**2. Interactive selection and navigation**

- Build a shared selector using `/dev/tty`, keeping selections on stdout and preserving original bytes separately from display text.
- Support cancellation, resizing, multiple selection, streamed candidates and terminal restoration.
- Add directory ranking, persistence, exclusions and shell adapters that change the parent shell’s directory without evaluating selected text.
- Provide opt-in bindings and completions; never edit shell startup files automatically.

**3. Encrypted history**

- Allow reviewed, pinned `argon2`, `chacha20poly1305`, `getrandom`, and `zeroize` crates plus their required transitive dependencies. Inventory all dependencies; do not implement cryptographic primitives ourselves.
- Use [Argon2id](https://docs.rs/argon2/latest/argon2/) with a random 16-byte salt, 64 MiB memory, three iterations, one lane, and a 32-byte derived key. Use [XChaCha20-Poly1305](https://docs.rs/chacha20poly1305/latest/chacha20poly1305/) with fresh random 24-byte nonces.
- Define a versioned vault format authenticating format metadata, vault identity and record sequence. Encrypt command text and associated history metadata. Bound KDF parameters and request sizes before allocating resources.
- Use one agent per user/vault to serialize concurrent shell writes. Keep keys in memory; accept passphrases through a terminal prompt, never arguments or environment variables.
- Require owner-only storage, symlink-resistant file handling, authenticated local socket peers and bounded messages. Reject corrupt/authentication-failing data without plaintext fallback.
- Lock explicitly, after the last registered shell exits, or after 15 minutes without activity. Detect dead shell registrations; treat logout cleanup as best effort.
- Enable recording explicitly. Preserve shell exit status and existing hooks; disable native history-file persistence in the enabled session. Leave existing plaintext history untouched.
- Provide exclusions before persistence and recall into the editable command line without automatic execution.

Encryption protects stored history; it does not protect an unlocked session from compromised same-user processes or root. Authentication does not guarantee detection of whole-vault rollback.

**4. Git integration**

- Invoke a fixed trusted Git executable with argument arrays and a controlled environment.
- Harden every operation against executable configuration, including hooks, filters, diff helpers, filesystem monitors, signing, editors and pagers. Disable network activity and submodule recursion.
- Reject unsupported configurations before writes. Add file staging, unstaging and commits only after inspection passes adversarial tests.
- Preview writes, recheck repository state and report failures without destructive recovery.

## Verification and acceptance

- Test hostile filenames and content: spaces, quotes, leading dashes, newlines, invalid UTF-8, terminal escapes and shell substitution text.
- Exercise real Bash/Zsh sessions for hooks, multiline history, concurrent shells, exclusions, cancellation and terminal restoration.
- Test JSON/regex malformed inputs, resource limits, symlink loops, ignore precedence, permission failures and broken pipes.
- Test encryption vectors, wrong passwords, tampering, truncated writes, crashes, nonce generation, concurrent unlocks and lock lifecycle.
- Use disposable Git repositories containing executable sentinel hooks/helpers/filters; require zero unexpected executions and verify actual index/ref results.
- Run on macOS and Linux:
  - `cargo fmt --check`
  - `cargo clippy --locked --offline --all-targets -- -D warnings`
  - `cargo test --locked --offline`
  - `cargo build --release --locked --offline`
- Review cryptographic dependencies before caching them for offline builds. Obtain fresh independent review of each stage’s diff and raw test evidence.
- Report exact commands, exit statuses, artifact paths and verification gaps. Make no upstream-performance or full-compatibility claims without evidence.

## Current baseline

The workspace is empty. Research and environment inspection are complete; no files were changed and no implementation tests have run. Installation, aliases and shell activation remain explicit actions after the toolkit is validated.

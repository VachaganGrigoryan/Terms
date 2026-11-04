# Phase 1 contract

Phase 1 implements original, standard-library-only listing, offline help, search and JSON processing. This document supersedes Phase 0's placeholder behavior for those four commands. The original roadmap remains unchanged. The other four commands remain unavailable. No installation or shell activation is performed.

## Shared execution and boundaries

Arguments retain Unix `OsString` bytes. Command execution writes byte-oriented output to injected streams instead of collecting entire output in UTF-8 response strings. Untrusted human-facing strings escape C0/C1 controls, DEL, ESC, bidi formatting controls and invalid UTF-8. Only our renderer may generate ANSI styling. Binary filename transport is explicit NUL output; JSON remains valid UTF-8. Commands do not execute examples or external processes and create no application state. Filesystem reads can still cause OS-managed access-time changes.

Global help/version succeed with exit 0. Implemented commands support their own `--help`. Unknown flags, missing values, unsupported combinations, invalid data, exhausted budgets and I/O failures return 2. Search/query/JSON streams with no results return 1. Valid false/null JSON results succeed. Partial results may precede an error; an error overrides successful results. Broken pipes fail without panics. `--` ends option parsing; short flags are separate, never combined. Option values are separate arguments.

This is read-only processing, not a filesystem sandbox. Path-based standard-library traversal cannot prevent malicious concurrent path replacement. Symlink policies mean links are not intentionally followed, not that races are impossible. Inputs may change; detected changes fail clearly, but complete snapshot consistency is not promised.

## Listing

`terms ls [--all] [--long] [--tree] [--level N] [--sort name|size|modified] [--reverse] [--header] [--color auto|never|always] [--icons none|emoji|nerd|phosphor] [--] [PATH...]`

Default is the current directory, one entry per line, hidden names excluded. Explicit hidden operands are still displayed. Names are ordered by raw bytes; size and modified sorts ascend, with name-byte ties (timestamp sorting retains nanosecond precision). Reverse reverses the entire sorted sequence. Operands retain argument order. Multiple non-tree directory operands have escaped path headings.

Long columns are type, four-digit octal permissions, numeric UID/GID, byte size, whole-second UTC RFC 3339 modification time, escaped name, and symlink target where present. Out-of-range timestamp years are errors. `--header` requires `--long`. Tree renders the root at depth zero, with ASCII branches; `--level` requires tree and accepts 0–256. Explicit level truncation succeeds; the implicit safety limit reports an error when recursion would continue. Hidden `.` and `..` pseudo-entries are never synthesized.

Symlinks, including broken links and directory-link operands, are displayed with their escaped target and never intentionally recursed into. Special files are shown from metadata only, never opened. Continue after individual operand/display errors, return 2 and retain at most 64 diagnostics plus an omission notice. Writer failure aborts immediately. A single directory is bounded to one million entries and 64 MiB of raw filename bytes; tree recursion is bounded to 256. Metadata-read failure stops that directory rather than silently omitting entries.

Auto color requires a terminal, `TERM` present and not `dumb`, and absent `NO_COLOR`. Explicit always/never overrides environment. Directories are blue and symlinks cyan. `--icons none|emoji|nerd|phosphor` (default none) prefixes each name with a glyph chosen by file type, well-known name (Dockerfile, lockfiles, README and similar), lowercase extension (about 70 kinds: languages, images, audio/video, archives, documents, fonts, binaries and more), then dotfile (config) and executable bit; anything else gets the plain file icon. Names and metadata only; files are never opened. An explicit flag always applies. The `TERMS_ICONS` environment variable takes the same set names as a default, but applies only when stdout is a terminal and `TERM` is not `dumb`; unset, empty, `none` or unknown values mean no icons and never cause an error. Emoji are single-codepoint wide characters and need no font. Phosphor (regular, web 2.1.1) and Nerd Font (v3.4.0 glyphnames) use private-use codepoints and need that font installed, otherwise replacement boxes appear. No dependency is added. Grid, directory grouping, hyperlinks, themes and Git status are deferred.

## Offline help

`terms help [--list | --search TEXT] [--platform macos|linux|common] [--plain] [--] [COMMAND...]`

Pages are original English Markdown embedded in the executable. Canonical topics are `terms ls`, `terms help`, `terms search`, `terms json`, `cd`, `pwd`, `mkdir`, `cp`, `mv`, `cat` and `find`. Bare `ls`, `help`, `search`, `json` are aliases for their `terms` topics. Separate command words are joined with spaces, whitespace normalized, and names otherwise match case-sensitively. Page names allow only ASCII letters/digits, spaces, `_` and `-`; names are limited to 1024 bytes. No paths are opened. No arguments display usage.

The selected platform defaults to the compiled host OS and takes precedence over a common page. No fallback to an unrelated platform. List uses unique sorted names. Search is ASCII case-insensitive substring matching over resolved page text (including title/examples), with a 64 KiB query limit. List/search are mutually exclusive and cannot include a topic operand. Unknown topics and empty search results return 1. Our inert renderer handles headings, paragraphs, bullets and fenced examples. Plain output has no ANSI; styled output requires terminal/color eligibility. No example runs, page download, cache or imported tldr corpus exists.

## Search and JSON

The Phase 0 regex/JSON grammar and parser limits remain binding except for these deliberate clarifications:

- Explicit positive search globs form a union and negative globs exclude regardless of order. Globs only further restrict paths accepted by ignore rules and never resurrect pruned directories.
- Search skips discovered links/special files; explicit links/special operands are errors. Search reads regular files only; `-` is a literal filename. Empty patterns match every text line. Counts are matching-line counts, including zero. Ordinary output uses escaped `path:content`, with optional line numbers; context merges overlaps and uses `-` separators.
- `--null` is restricted to matching-filename/file-enumeration modes, preserving original path bytes. Structured output is deferred.
- Search classifies the entire opened descriptor for NUL before matching, rewinds, and compares metadata before/after. Same-size concurrent rewrites can evade detection; output is not transactional. NFA processing is limited to 100 million state visits per file. Literal matching is linear-time.
- JSON files are explicit paths and `-` is literal; no operands means stdin. Output preserves input object order and numeric tokens, with two-space pretty indentation or compact mode and one newline per result.
- Raw string output preserves exact UTF-8 bytes to pipes/files; terminal output visibly escapes controls. Nonstring raw results remain JSON. Numeric equality normalizes decimal tokens without exponent-sized allocation or floating point.
- In addition to Phase 0 limits, JSON generated output is capped at 64 MiB per input value. Earlier values may have been emitted before a later input fails.

## Review and verification

Each implemented milestone receives focused tests and fresh independent review. Process tests cover binary-safe arguments/output, correct statuses, no application-state creation, hostile controls/filenames, symlinks, errors, resource boundaries and malformed inputs. Source research at immutable revisions is recorded in research.md; reading a manual is not a source audit or security guarantee.

Required checks: `cargo fmt --check`; `cargo clippy --locked --offline --all-targets -- -D warnings`; `cargo test --locked --offline`; `cargo build --release --locked --offline`. Record actual statuses, host/toolchain and artifact paths. Linux and Rust minimum-version validation require actual environments, not inference from a newer macOS compiler. Shell hooks, PTYs, history cryptography and Git integration remain later phases.

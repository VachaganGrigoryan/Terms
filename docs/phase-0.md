# Phase 0 implementation and security contract

Status: specification and CLI bootstrap. The roadmap remains the original planning baseline; its historical “empty workspace” paragraph is not a current implementation report. Research evidence and limitations are in [research.md](research.md).

## Phase 0 public interface

Only `terms --help` (alias `-h`) and `terms --version` (alias `-V`) execute functionality, printing to stdout and returning 0. No arguments prints help and returns 0. `ls`, `jump`, `pick`, `search`, `help`, `history`, `git`, and `json` are advertised as planned commands. Invoking any, with or without extra arguments, returns 2 with an unavailable diagnostic on stderr and no stdout. `terms help` is deliberately not an alias for `--help` during this phase.

Unknown commands/options, extra arguments to global help/version, and non-UTF-8 command tokens return 2 without echoing raw hostile bytes. No configuration, stdin, filesystem data, shell startup files or history is read by the CLI. No runtime state is created. I/O failure returns 2. The binary does not start processes or use networking/FFI.

The package is one binary with a thin entrypoint and a testable library. No third-party dependencies, build scripts or toolchain modifications. Cargo lockfile is retained. Build artifacts are ignored. This bootstrap must not claim any of the eight tools is implemented.

## Trust and runtime policy

Trusted foundations are the selected Rust compiler/standard library, OS/kernel/native APIs, and Bash/Zsh for opted-in integrations. Future Git support additionally trusts a fixed explicitly configured absolute system-Git path, not a repository-controlled PATH lookup. The eight originals and their libraries are not runtime dependencies.

Third-party Rust dependencies are prohibited except reviewed cryptography crates and required transitive dependencies in the history phase. No crates are added now. Future native unsafe code stays in small platform modules with safety invariants and focused independent review. No automatic updates, telemetry, sync, downloads, arbitrary executable configuration, shell evaluation, or implicit activation. No aliases or startup files are changed by installation.

Protect against untrusted files, terminal escape injection, malformed/oversized inputs, malicious repository configuration and accidental disclosure of stored history. This is not protection from a compromised compiler, kernel, root or same-user attacker controlling an unlocked session. Encryption alone cannot establish trust in the rest of the application.

## Future command contracts

These are design commitments for later phases, not accepted options in Phase 0. Any extension must update this contract before implementation. Shared future exit codes: 0 successful operation; 1 no search/query/selection results; 2 usage/data/I/O/resource/unsupported errors; 130 interactive cancellation. JSON succeeds with 0 for valid false/null output; empty output streams return 1. No partial write is represented as success.

| Command | First implemented interface |
|---|---|
| `ls` | `[--all] [--long] [--tree] [--level N] [--sort name\|size\|modified] [--reverse] [--header] [--color auto\|never\|always] [--] [PATH...]`; default current directory, name sort, hidden excluded |
| `help` | `[--list] [--search TEXT] [--platform macos\|linux\|common] [--plain] [--] [COMMAND...]`; authored English pages, joined multiword names, platform page before common; no path traversal |
| `search` | `[-F] [-i] [-n] [-l] [-c] [--files] [--hidden] [--no-ignore] [--glob PATTERN] [--context N] [--null] [--] PATTERN [PATH...]`; literal with `-F`, bounded regex otherwise; current directory by default; `--files` takes paths and no pattern |
| `json` | `[-c] [-r] [--] FILTER [FILE...]`; stdin if no file, explicit files only, JSON values separated by whitespace; raw output only for string results |
| `pick` | `[--exact] [--multi] [--read0] [--print0] [--query TEXT]`; records from stdin, UI on `/dev/tty`, selected original records on stdout |
| `jump` | `add PATH`, `remove PATH`, `query [--interactive] [--null] [--] TERM...`; only static shell adapters perform directory changes |
| `history` | `unlock`, `lock`, `status`, `search`, `import FILE`, `export FILE`, `delete`, `retention`; operational formats and encrypted agent protocol must pass the history gate before implementation |
| `git` | Inspection first; exact operation argv/environment and configuration-rejection policy must pass the Git gate before implementation |

Do not silently ignore unknown flags or unsupported syntax. Options accepting values reject missing/out-of-range values. `--` ends option parsing. Compact short-option combinations are not initially supported. Mutually exclusive output modes (`search -l`/`-c`/`--files`, for example) are errors. Later grid/icons/Git listing and advanced commands require explicit contract additions.

### Bytes, presentation and traversal

Preserve Unix path bytes separately from display strings. Escape C0/C1 controls, ESC, DEL, invalid UTF-8 bytes and bidi formatting controls in human output; do not accept embedded terminal escapes as trusted styling. Machine filename transport is NUL-delimited when requested, preserving bytes exactly. JSON output remains valid UTF-8 with JSON escapes. Never use selected content as shell code. Shell adapters use quoted data, including embedded and trailing newlines, without command substitution that strips them.

Do not recurse through directory symlinks. Explicit root directory symlinks are rejected for recursion unless a later documented option permits them; listing displays links themselves. Search excludes hidden path components and `.git` directories by default; `--hidden` includes hidden files but still excludes `.git`. Explicit regular-file inputs bypass hidden/ignore filtering. Binary means NUL detected anywhere in a streamed file; suppress its match output until classification is complete. Do not emit speculative matches then retract them. Implement a bounded spool or two-pass strategy and diagnose files changing during processing.

### Ignore semantics

For recursive search read `.gitignore` then `.ignore` in each traversed directory. Parent rules precede child rules; within a file the last matching rule wins; `.ignore` wins over `.gitignore` at the same depth. Read no global Git excludes or executable config. `--no-ignore` disables these files; explicit `--glob` filters are evaluated last, in argument order.

Support blank/comment lines, `!` negation, escaped leading `#`/`!`, escaped trailing spaces, basename patterns without slash, directory-relative slash patterns, root anchoring `/`, directory suffix `/`, `*`, `?`, bracket classes, and whole-component `**`. `*`/`?` do not cross `/`; `**` does. Excluded directories are pruned: a child cannot be re-included without re-including its ancestors. Unsupported/malformed patterns produce a diagnostic and exit 2 rather than silently broadening search. User-supplied positive globs restrict matches to their union; negated globs exclude. Parent paths outside explicitly selected roots are not read. This subset is our contract, not a claim of complete Git compatibility.

### Regex and JSON subsets

Regex is byte-oriented Thompson NFA: concatenation, `|`, `(...)`, `.`, `^`, `$`, `*`, `+`, `?`, ASCII bracket ranges/negation and backslash-escaped metacharacters. Dot excludes LF. UTF-8 literal text matches its encoded bytes. Case-insensitive matching folds ASCII only. Reject backreferences, lookaround, counted/lazy repetition, Unicode properties and unknown escapes. Limit patterns to 64 KiB, nesting to 128 and compiled states to 16,384; no backtracking engine.

JSON filter grammar initially supports identity `.`, chained `.field`/`["field"]`/nonnegative `[index]`, `.[]`, pipes, `select(PATH == LITERAL)` and `select(PATH != LITERAL)`, `[FILTER]`, `{key: FILTER, ...}`, `map(FILTER)` and `length`. Field names are ASCII identifiers or quoted JSON strings; literals are JSON scalar values. Constructors evaluate object field filters against the same input and require exactly one result per field. Iteration supports arrays and objects in input order. Missing fields/out-of-range indexes yield null; invalid operand types fail. Equality uses JSON type/value equality, with numeric comparison implemented on decimal tokens without floating point; no arithmetic. Object equality ignores key order, and arrays compare in order. No implicit environment, modules or files.

Parse strict UTF-8 JSON, valid escape/surrogate pairs, finite JSON number grammar, no duplicate object keys, and preserve original numeric tokens on output. `length` accepts arrays/objects/strings (Unicode scalar count), not numbers. Limit nesting to 128, filter bytes to 64 KiB, one input JSON value to 64 MiB, and evaluation work to one million node visits per value. Exhaustion fails with exit 2. Input/output streams may have multiple values; already emitted values cannot be rolled back on a later error.

General limits: ignore file 1 MiB, input record/search line 16 MiB, traversal depth 256, interactive candidate bytes 64 MiB. Oversized input fails clearly rather than truncating. Cap requested context at 1,000 lines and depth options at 256. Document any eventual configurable increases before adding them.

## Sensitive history gate (Phase 3)

Use Argon2id (16-byte random salt, 64 MiB, three iterations, one lane, 32-byte key), XChaCha20-Poly1305 with fresh random 24-byte nonces, OS randomness and key zeroization through reviewed pinned crates. No homegrown primitives. Freeze a versioned authenticated vault/protocol specification before writing storage code, including authenticated vault identity/sequence, KDF parameter bounds, framing limits, concurrent append serialization, crash recovery and truncated-record handling. Fail closed on authentication errors; never silently repair by discarding authenticated data.

One agent per user/vault serializes writes. Validate socket peers and file ownership, private directory mode 0700/file mode 0600, and symlink-resistant creation/opening. Passphrases arrive through a terminal, never argv/environment; key stays in memory. Explicit lock, last live registered shell exit or 15 minutes idle clears the key. Specify dead-registration detection and test it; logout cleanup is best effort. Exclusions apply before persistence. Encryption authenticates data but does not guarantee detection of whole-vault rollback/deletion or protection from unlocked memory, swap/core dumps, backups or a same-user attacker.

Adapters must preserve exit status, coexist with hooks without evaluating configuration, handle multiline commands, and not collect bookkeeping calls. Opt-in managed sessions disable native history-file persistence; existing plaintext history is neither imported nor removed implicitly. Recall inserts text for editing without executing it. Plaintext export requires an explicit target/action; no plaintext temporary files. Missing unlock skips persistence with a visible locked status and no plaintext fallback.

## Git gate (Phase 4)

Before any implementation, record exact trusted executable path, operation argument arrays and environment allowlist. Disable inherited Git config injection, globals/system config, hooks, signing, fsmonitor, pager/editor, external diffs/text conversion, submodule recursion, auto maintenance and network actions. Inspect/reject unsupported repository config includes, attributes and filter drivers before writes; preflight alone is not a sandbox and must not be described as one. Reject unsupported configurations before changing the index.

Test each operation using sentinel executables in helper/hook/filter locations, adversarial environment/config and concurrent repository changes. Require no unexpected execution. Inspection uses machine-readable NUL-safe results; names/messages/diffs are escaped. File staging/unstaging and explicit-message commits follow only after inspection passes. No checkout, stash mutation, destructive recovery, history rewriting or remote operations initially.

## Acceptance and remaining verification

Phase 0 requires process-level tests for global help/version, all unavailable subcommands, unknown/hostile tokens, stdout/stderr separation, exit codes and absence of application-state changes. Run fmt check, offline locked strict Clippy, offline locked tests and offline locked release build. Record exact commands, statuses, artifact paths and toolchain in the completion report; do not infer Linux success from macOS. Independent review receives the contract, baseline/diff and raw results.

Unresolved source-level execution-surface verification blocks affected features, not the bootstrap. History vault/protocol and operation-specific Git hardening are intentional later gates; no unreviewed implementation may fill them implicitly. Crypto dependencies are not approved by merely naming algorithms. Linux, real shell hooks, PTYs, cryptography and hostile Git behavior remain unverified until actually implemented and tested.

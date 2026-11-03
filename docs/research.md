# Terms research inventory

Research date: 2026-11-03. This is a capability study, not an audit or a claim that the original tools are malicious. Terms will implement original code and examples; it will not wrap these eight binaries. No upstream tools were installed or executed.

## Evidence and reproducibility

The official pages linked below were inspected through the web tool. GitHub HEAD identifiers were obtained with `gh api repos/OWNER/REPO/commits/HEAD --jq .sha` (all eight requests exited 0). They identify observed upstream state, **not a pinned source audit**. Subsequent attempts to fetch README contents at those revisions failed with network errors (exit 1). Consequently branch-linked documentation may differ from the observed revisions; source-level verification remains a gate for affected features. No claims about implementation internals rely on those unread files.

| Project | Observed HEAD (not audited) | Primary evidence inspected |
|---|---|---|
| eza | `471bfbc7b03cbac8c738e8d9050edb06ee79132a` | [Official README](https://github.com/eza-community/eza), [manual](https://github.com/eza-community/eza/blob/main/man/eza.1.md) |
| zoxide | `f0b31631d295b0623bc440016ed0aba3c7c49dde` | [Official README](https://github.com/ajeetdsouza/zoxide) |
| fzf | `99bfd5aa0f66133d7dcfbf18f8044eb765f04649` | [Official README](https://github.com/junegunn/fzf), [manual](https://github.com/junegunn/fzf/blob/master/man/man1/fzf.1) |
| ripgrep | `91f5d22ef48bde459d90a8c7dbeab416e97d0f2b` | [Official README](https://github.com/BurntSushi/ripgrep), [guide](https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md) |
| tldr | `4667b5f5391d971266776c6532ed9be56b490b1c` | [Client specification](https://github.com/tldr-pages/tldr/blob/main/CLIENT-SPECIFICATION.md) |
| Atuin | `8edce244deb9f622c4fa58260fba0bf1bbbc7a4e` | [Official README](https://github.com/atuinsh/atuin), [basic usage](https://docs.atuin.sh/latest/guide/basic-usage/) |
| lazygit | `c9c0db40bd47894e95c4fdaab0f3eadb40edb55c` | [Official README](https://github.com/jesseduffield/lazygit), [configuration](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md) |
| jq | `5c001b18d7237cce1fdba63608732163f978dd23` | [Official README](https://github.com/jqlang/jq), [jq 1.8 manual](https://jqlang.org/manual/) |

Repository revisions and the published jq 1.8 manual are separate references. Neither promises byte-for-byte compatibility. This inventory groups documented capabilities; it is not an exhaustive enumeration of every option or plugin.

## Capability mapping

“Planned” means a future implementation commitment, not functionality present in Phase 0. All eight commands are unavailable in Phase 0.

| Reference → Terms | Planned | Deferred | Excluded from initial product |
|---|---|---|---|
| eza → `ls` | One-line/grid/long/tree views; hidden files; depth; sort/reverse; directory grouping; permissions, size/time metadata; headers; icons; symlink display; Git status through hardened adapter | Extended attributes; mount/security context; custom themes; recursive sizes; hyperlinks; extra timestamp formats | Executable themes or configuration |
| zoxide → `jump` | Visit recording; multi-term ranked queries; frequency/recency; interactive choice; direct paths; exclusions/removal; Bash/Zsh completion | Importing third-party databases; additional shells | Runtime reliance on zoxide/fzf; generated shell code evaluated at startup |
| fzf → `pick` | Fuzzy/exact search; ASCII smart case; deterministic ranking; navigation; multi-select; field selection; NUL records; separate terminal UI | Rich query/action language; custom layouts; built-in read-only previews | Shell previews; execute/reload actions; HTTP control server |
| ripgrep → `search` | Recursive literal/regex search; globs/ignore rules; case modes; line numbers/context; counts; files-only/listing; human/structured output | Multiline matching; Unicode case tables; encodings; parallel optimization | PCRE2; compressed search; executable preprocessors; implicit upstream config |
| tldr → `help` | Offline original examples; platform/common lookup; multiword pages; listing/search; plain/formatted rendering | More languages; larger authored corpus | Automatic downloads; executing examples; imported unreviewed corpus |
| Atuin → `history` | Opt-in encrypted command text and cwd/session/time/duration/status; filtering; interactive recall; exclusions; deletion/retention; explicit import/export | Statistics; third-party import formats; key-store integrations | Sync/accounts; telemetry; AI features; output recording; plaintext fallback |
| lazygit → `git` | Status/diff/log/branches/stash inspection; later file staging/unstaging and explicit-message commits | Partial staging; checkout; stash mutation; conflicts; worktrees; rebase/cherry-pick; reflog workflows | Automatic fetch/update; remote operations initially; arbitrary custom commands |
| jq → `json` | JSON formatting/validation; path/index/iteration; pipelines; select; projection; map/length; raw/compact output; value streams | Arithmetic; sorting/grouping/reduction; updates; variables; event streaming; wider builtins | Implicit modules; environment access; implicit files; user code outside supported grammar |

## Security-relevant behavior and our response

These are engineering consequences of the referenced capabilities, not vulnerability findings.

| Tool | Files/state and execution surfaces | Dependency boundary and Terms response | Acceptance scenario |
|---|---|---|---|
| eza | Traversal reads filenames/metadata; optional Git and themes; filenames reach terminal | Own traversal/rendering, OS metadata, shared hardened system-Git adapter only | Escape hostile names; handle unreadable entries and symlink loops; no executable theme loading |
| zoxide | Shell hooks persist visited paths; a shell function performs `cd` | Own store/ranking and static shell adapters; no external selector | Exclusions prevent persistence; selected newline-containing paths remain exact and never execute |
| fzf | Reads candidates and terminal; optional bindings/previews can run commands | Own selector/terminal layer, no shell action language | Piped/NUL records round-trip; cancel emits no selection; terminal restored |
| ripgrep | Reads content/ignore configuration; optional preprocessing launches programs | Own traversal, ignore and bounded NFA; no original crates or binary | Ignore precedence, binary/hidden policy, malformed regex, broken pipe and oversized input tests |
| tldr | Client reads example corpus; client ecosystem supports cache updates | Original bundled inert Markdown; no downloader or imported corpus dependency | Works offline; traversal-like page names rejected; examples never run |
| Atuin | Shell hooks collect sensitive history; ecosystem includes sync and optional output capture | Own local agent/storage; approved crypto-only crate exception | Wrong key/tampering fail closed; concurrent sessions preserve metadata; excluded commands never persist |
| lazygit | Git subprocesses; configuration/custom commands/editors; Git itself can run helpers and filters | Fixed system Git, controlled environment and operation-specific hardening | Sentinel hooks/filter/helpers never execute; rejected writes leave index unchanged |
| jq | Reads JSON; language/options may access environment, modules and explicit files | Original bounded parser/interpreter; no jq/serde dependency | Screenshot filter works; huge integers preserved; invalid Unicode/deep nesting fail clearly |

## Research and review gates

- Before implementing a feature, retrieve and inspect the relevant source/manual at an immutable revision and update this inventory. HEAD lookup alone is insufficient.
- Verify command-execution surfaces against source, particularly Git inherited environment, config includes, attributes/filters and platform defaults. A config preflight alone is insufficient protection against races or later execution paths.
- Review every proposed crypto dependency and transitive dependency for provenance, license, advisories, build scripts and unsafe code; record exact versions/checksums. No crypto packages are needed in Phase 0.
- Retain original golden fixtures and generated adversarial cases. Optional upstream comparisons require isolated later research; originals are never runtime dependencies.
- Performance parity and security superiority remain unverified. Research does not replace independent review, fuzz-style generated testing or actual host verification.

## Phase 1 immutable manual inspection

On 2026-11-03, the eza manual at `471bfbc7b03cbac8c738e8d9050edb06ee79132a/man/eza.1.md` and tldr client specification at `4667b5f5391d971266776c6532ed9be56b490b1c/CLIENT-SPECIFICATION.md` were retrieved and read in full through GitHub's contents API (`gh api 'repos/OWNER/REPO/contents/PATH?ref=REVISION' -H 'Accept: application/vnd.github.raw+json'`, exit 0 for each). Raw web fetches failed with cache misses; the API retrieval resolved that manual-evidence gap. An initial unquoted shell URL failed before execution; the corrected quoted API calls succeeded. No upstream binaries were installed or run, and no implementation code was copied.

- [Pinned eza manual](https://github.com/eza-community/eza/blob/471bfbc7b03cbac8c738e8d9050edb06ee79132a/man/eza.1.md): inspected display/tree/depth, metadata/sort, symlink, color/environment and exit-status options. Terms deliberately uses a smaller interface, default byte-name order, numeric IDs, UTC timestamps and uniform error status 2. It ignores theme/executable integration surfaces and does not perform Git operations. This establishes documented behavior, not a complete upstream source audit.
- [Pinned tldr client specification](https://github.com/tldr-pages/tldr/blob/4667b5f5391d971266776c6532ed9be56b490b1c/CLIENT-SPECIFICATION.md): inspected platform/common resolution, multiword names, inert examples and optional caching. Terms uses authored embedded examples and exact canonical names rather than claiming specification compliance; it has no cache or downloader. No tldr corpus is imported.

Phase 1 listing uses one-line/long/tree output, sorting and color only. Grid, icons, directory grouping and Git status remain deferred. Phosphor's [official core repository](https://github.com/phosphor-icons/core) supplies SVG assets/catalog data; use in a text terminal needs a separately selected compatible font. No icon package is added in Phase 1.

### Phase 1 search and JSON evidence

The search specialist retrieved and inspected `GUIDE.md` at ripgrep revision `91f5d22ef48bde459d90a8c7dbeab416e97d0f2b` using GitHub API (exit 0). Pinned ignore-source retrieval failed with a network error; source-specific ignore verification remains incomplete.

The JSON specialist inspected the published jq 1.8 manual at https://jqlang.org/manual/v1.8/ for streams, output, filters and numeric behavior. Immutable commit source/manual retrieval failed through web and GitHub API. The coordinator repeated `gh api 'repos/jqlang/jq/contents/src/jv_parse.c?ref=5c001b18d7237cce1fdba63608732163f978dd23' -H 'Accept: application/vnd.github.raw+json'` and received exit 1 (network error). The immutable-source research gate for JSON remains OPEN. Its implementation and passing tests do not satisfy this gate; Phase 1 must not be represented as fully accepted until it closes.

### Phase 2 shell evidence

Installed Bash 3.2 `help bind` was inspected (command exit0). It documents `bind -x` but no `bind -X`; therefore ownership introspection/removal is not claimed on this older shell. Installed Bash and Zsh parser/process tests passed as recorded in the Phase 2 verification report. Official documentation references for further review are https://www.gnu.org/software/bash/manual/html_node/Bash-Builtins.html and https://zsh.sourceforge.io/Doc/Release/Zsh-Line-Editor.html. Immutable shell-source retrieval is not yet established by this evidence. Real PTY/Readline/ZLE and modern-Bash verification remain separate gates.

## Phase 2 source-verification limits

Pinned fzf manual retrieval via GitHub API failed with a network error. Pinned zoxide README retrieval via GitHub API failed with exit1; web immutable-reference retrieval returned cache miss. Neither gate is closed by the original capability inventory. Native ABI/layout review and implementation tests are separate evidence, not upstream-source audit evidence. See phase-2-verification.md for exact acceptance status.

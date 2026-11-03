# Phase 1 verification

Implementation is present; acceptance remains incomplete because immutable jq source verification, Linux and minimum-compiler validation are unavailable. Original roadmap unchanged; Phase 0 documents remain historical references.

Implemented commands: ls, help, search, json. One dependency-free Rust package; no installed aliases, startup changes, history capture, services or upstream executables. Streaming orchestration replaces the previous Response buffer API with execute(args, input, output, terminal).

## Final local checks

macOS workstation, installed Rust/Cargo 1.99.1:

| Exact command | Exit | Result |
|---|---|---|
| `cargo fmt --check` | 0 | Passed |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 0 | Passed |
| `cargo test --locked --offline` | 0 | 17 unit tests + 4 process tests passed |
| `cargo build --release --locked --offline` | 0 | `target/release/terms` |

Additional Python subprocess probes passed (harness exit 0): ten JSON identity round-trips through the release executable and four search cases covering matches, no matches, malformed regex, context and late-NUL binary exclusion.

Independent reviewer found a JSON generated-depth stack overflow. Fixed by iterative validation of every intermediate result against depth 128 and the evaluation budget. Added the 5,000-stage constructor pipeline process regression. Reviewer reran `cargo test --locked --offline json_constructed_depth_is_bounded` (exit 0) and verified exit 2 instead of abort. No additional findings reported. Final full checks above ran after the fix.

## Limitations

- Immutable jq source research gate remains open; versioned manual inspected. Pinned ripgrep guide inspected but ignore-source retrieval failed. See research.md for source provenance and successful eza/tldr inspection.
- Linux and Rust 1.85 not tested. macOS refused creation of invalid-UTF-8 filenames; raw-byte escaping has unit coverage and newline filenames have filesystem coverage.
- Filesystem symlink policy is best effort with path-based std APIs, not a race-resistant sandbox. Search binary classification assumes stable files and detects metadata changes best effort.
- Human output intentionally differs from upstream tools; JSON raw output escapes controls on terminals but preserves bytes in pipes/files.
- Permission failures, real-terminal rendering and broken pipes do not have comprehensive process coverage. Listing writer failure has focused coverage.
- Original research review and tests do not establish security superiority or upstream performance parity.

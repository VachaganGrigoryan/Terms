# History vault v1 implementation review record

Status: implementation and local tests, **not an independent cryptographic audit**. Dependency inventory/checksums and unresolved advisory evidence are recorded by the coordinator in history-dependencies.md. Exact direct versions inspected locally: argon20.5.3, chacha20poly1305 0.11.0, getrandom0.4.3, zeroize1.9.1. The cached manifests specify dual MIT/Apache2 licensing, Rust floors1.65/1.85/1.85/1.85 respectively. ChaCha default getrandom feature is disabled; only alloc/zeroize enabled. Argon2 uses alloc/zeroize. getrandom has a build script; its existence must not be mistaken for an absent execution surface. These checks are an inventory, not proof of provenance or a completed transitive source audit.

## Exact container

All integers little-endian. Header is56 bytes: magic TERMSHV1(8), random vaultID(16), random salt(16), Argon2 memoryKiB u32=65536, iterationsu32=3, parallelismu32=1, outputbytesu32=32. Only this tuple/version is accepted. Argon2id version19 derives32bytes. Header inspection reads only56bytes before derivation.

Each frame: u32 body length, random XChaCha nonce24, ciphertext, authenticationtag16. Body length must be40..1048616 inclusive; plaintext at most1MiB. Sequence starts0. AAD is header56 + sequenceu64 + previous tag16 + bodylengthu32. Initial previous tag is16zero bytes. Frame0 plaintext is the fixed ASCII check `Terms history authenticated check v1`; it is mandatory even for empty vaults. Subsequent plaintexts are opaque agent event encodings. Sequence counters are not stored separately. Fresh nonces come exclusively from OS-backed getrandom; errors fail closed.

Vault cap256MiB, payload index cap64MiB, event count cap100000. Caller must serialize operations with the lifetime agent lock and enforce stronger record/session policies. Appending verifies the complete existing chain before writing and syncs before returning. load authenticates all frames before returning any payload. Temporary plaintexts from verification are zeroized on handled errors; callers must zeroize returned payloads when finished.

Rewrite preserves salt/key but generates a new vaultID and fresh frame nonces, writes only encrypted bytes to an exclusive random same-directory temp file, syncs before rename, then syncs the directory. No plaintext temporary files. Error cleanup removes only the operation's own temp file.

Repair is explicit: authenticate header/check and every complete preceding frame; truncate only an incomplete final length/body, never a complete frame that fails authentication. A malformed complete length is rejected. Internal reordering/removal, different-vault frame splicing, header edits and payload corruption fail authentication. Valid suffix removal and whole-vault rollback cannot be detected without an independent trusted anchor. Deletion/compaction cannot erase old snapshots/backups.

## Filesystem and memory boundary

Final file opens use nofollow/nonblock and validate regular-file type, effective owner and0600 mode. Parent directory ownership/private permissions and lifetime locking belong to the agent. Path-based ancestor replacement is not a dirfd sandbox. No plaintext-log output exists in this module. Passphrases, keys and transient raw payloads are the caller's responsibility at API boundaries; derived keys are returned as Zeroizing<Vec<u8>>. Core dumps, swap, same-user compromise, allocation copies and compiler/OS compromise are not solved by this module.

## Validation

Isolated scratch manifest /tmp/terms-vault-validation points its library at src/history/vault.rs using the same pinned dependencies. `cargo test --manifest-path /tmp/terms-vault-validation/Cargo.toml --offline` passed the authenticated roundtrip/wrong-key/rewrite/truncated-tail/tamper test. `cargo clippy --manifest-path /tmp/terms-vault-validation/Cargo.toml --offline --all-targets -- -D warnings` passed. Full integration, independent format review, external known-answer vectors, filesystem crash injection and random-failure injection remain additional gates. No claim of completed advisory review is made.

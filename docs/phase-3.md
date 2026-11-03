# Phase 3: encrypted local command history

This is the Phase 3 implementation contract. It is not an acceptance claim. See phase-3-verification.md for checks actually run and unresolved gaps. Earlier phases' open gates remain open. The original roadmap is unchanged.

## Trust and dependency boundary

Only reviewed cryptographic crates and their required dependencies may extend the original std-only boundary. The approved algorithm suite is Argon2id v19 (16-byte random salt, 64 MiB, three iterations, one lane, 32-byte key), XChaCha20-Poly1305 with fresh random 24-byte nonces, OS-backed randomness and explicit zeroization. Exact dependency versions, checksums, features, licenses, build/unsafe surfaces, source-review evidence and advisory limitations belong in history-crypto-review.md. Naming an algorithm does not satisfy this gate.

No cloud accounts, synchronization, telemetry, automatic downloads, command-output capture, implicit imports, or plaintext fallback. Shell memory, unlocked same-user compromise, root, terminal capture, swap, core dumps and backups are outside the at-rest encryption guarantee. Authentication cannot prove that a complete vault or valid suffix was not rolled back/deleted.

## Vault and agent requirements

History state is separate from the existing plaintext directory-navigation database. Do not migrate or encrypt directory tracking implicitly. Use owner-only directories/files and a private Unix socket, no-follow/nonblocking final-file opens, ownership/type checks, exclusive lifetime locking, bounded operations and authenticated same-user peers.

A v1 vault must authenticate its version, random vault identity, fixed KDF tuple and salt, and include an encrypted check frame for empty-vault password verification. Length-prefixed encrypted event frames bind header/vault identity, sequence, preceding tag and length. All command metadata is encrypted. Framing leaks approximate volume. Native/format review and known-answer tests precede acceptance.

Starts and completions have stable random command/session IDs. A durable start lacking completion remains incomplete; never invent success/duration. Start/completion acknowledgment follows durable encrypted append. Bound commands64KiB, directories16KiB, requests/frames1MiB, vault256MiB, plaintext index64MiB. Retention defaults90days/10,000 commands. Compaction uses encrypted same-directory temporary storage, fresh nonces, sync/rename/directory-sync, serializes against append, and preserves IDs. No plaintext temporary files.

Foreground init/unlock reads a no-echo terminal, confirms initialization passphrases, derives the key and passes it over authenticated private IPC. Never accept passphrases or keys in argv/environment. The agent authenticates the complete vault before exposing records. Locking rejects sensitive requests and clears key/index/pending plaintext/results; no deferred plaintext replay. Lock after explicit request, last live registered shell exit, or15minutes of inactivity. PID plus process-start identity protects against reuse; five-second lifecycle checks do not extend idle timeout. Long commands that outlast lock retain only their authenticated start.

The agent must bound connections16, queued requests32 and sessions128. Invalid KDF tuples and request sizes fail before expensive work. Never remove a live/unverified existing socket. Shutdown/socket cleanup and unsupported native cases must be tested, not assumed.

Authentication errors fail closed. Explicit repair may truncate only an incomplete final frame after authenticating the preceding prefix; never discard a complete invalid frame silently. Delete/retention removes records from the live vault, without secure-erasure guarantees for snapshots/backups.

## CLI and shell framing

Public operations are init/unlock/lock/status/agent, search/pick, explicit import/export, delete/clear, retention/exclusion management, and explicit truncated-tail repair. `history pick --hex` returns one selected command as hexadecimal bytes without executing it. Empty/no-result is1, success0, error2, cancellation130. Status must not disclose commands while locked. Plaintext export requires an explicit target/action and must create a new private regular file without overwriting.

Hidden capture endpoints carry sensitive fields on stdin, never argv/environment. Every field is ASCII followed by LF:

| Invocation | stdin fields, in order | stdout on success |
|---|---|---|
| `terms history session-open` | Decimal parent-shell PID | Hexadecimal session ID + LF |
| `terms history begin` | Session ID; hex cwd bytes; hex complete submitted command bytes | Hexadecimal command ID + LF |
| `terms history finish` | Session ID; command ID; decimal exit status | No sensitive output |
| `terms history session-close` | Session ID | No sensitive output |

Reject extra/missing fields, invalid hex/IDs, embedded NUL in shell data, unsupported status values and over-limit payloads. Agent-side timestamps determine start/duration; lifecycle checks validate registration identity independently. The internal Unix-wire format must be documented with the implementing agent and covered by malformed-message tests; this shell framing is not itself peer authentication.

## Explicit shell activation

Source `shell/history.zsh` or `shell/history.bash`, or run `eval "$(terms init zsh|bash --history)"` (which prints the same adapters embedded in the binary), set TERMS_BIN to the trusted absolute executable if necessary, then call `terms_history_enable`. Sourcing or `terms init --history` only defines functions; capture is never enabled for the user. Automatic capture requires interactive Zsh or Bash4.4+. Bash3.2 refuses with exit2 and leaves history CLI usable.

Activation registers a session, disables native history-file saving by unsetting HISTFILE and installs idempotent hooks. Native persistence stays disabled after `terms_history_disable` until that shell exits: restoring HISTFILE could replay encrypted-session commands from memory into plaintext. Existing files are neither imported nor deleted. Users manually overriding HISTFILE or explicitly writing shell history are outside this protection.

Zsh chains preexec/precmd/zshexit arrays, retaining existing hooks. Bash refuses pre-existing DEBUG/EXIT traps, array PROMPT_COMMAND, functrace/extdebug and readonly HISTFILE rather than replacing incompatible behavior. Its prompt chain preserves the scalar existing command. Bash temporarily clears HISTCONTROL/HISTIGNORE and enables multiline in-memory history so `fc` refers to the actual submitted command; these changes must be validated on modern Bash before claiming support. Commands excluded from Terms can still exist in shell memory.

Capture each submitted multiline command once, with start directory, start/completion time and shell completion status. Background submissions record immediate shell status, not eventual job completion. Leading-space commands and Terms bookkeeping are excluded before IPC. Additional command-prefix/directory exclusions apply in the agent before persistence. Empty IDs, locked agent or persistence failures produce a visible state transition diagnostic, preserve shell status and never queue plaintext. Resume recording only after an acknowledged successful begin.

`terms_history_bindings_enable` separately installs editable recall only if Ctrl-R is unoccupied; users must unbind it explicitly otherwise. Recall validates hexadecimal transport, preserves newlines, replaces the editable command buffer and never submits it. Disable removes only a binding still owned by Terms. Real Readline/ZLE validation is mandatory; fixture invocation cannot establish terminal behavior.

## Required acceptance evidence

Run offline fmt/strictClippy/tests/release plus macOS/Linux agent, credential/socket and lifecycle tests, real Zsh/modern-Bash/PT Y tests, crypto known-answer/tamper/truncation/randomness-failure tests, crash/concurrent append-compaction cases and native-history replay tests. Independently review crypto formats, native unsafe/IPC and shell integration using actual diffs and raw results. No passing test implies security superiority or complete upstream compatibility.

`python3 shell/tests/history_adapters.py` uses an inert fixture executable and a real installed interactive Zsh process to check multiline capture, duplicate activation, exit-status preservation, leading-space exclusion, framing and enable→sensitive command→disable→exit without native-history replay. It also checks installed Bash3.2 refuses automatic capture. It does not validate cryptography, a live agent, PTY/Readline/ZLE, modern Bash, Linux or all hook combinations. Missing tools/permissions remain verification gaps.

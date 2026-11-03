# History protocol v1

Transport: private peer-credential-checked Unix stream. Each message starts with a u32 big-endian body length, maximum1MiB. Request body is `TH1\nOP\nPAYLOAD`. Response body starts status byte0(success) or2(error), then payload. Server whole-frame read/write deadlines are1s; client response frame deadline15s. Invalid lengths/version/operations fail. No sensitive argv/environment values are accepted.

Capture payloads are newline-delimited: session-open PID; begin sessionID,cwdHex,commandHex; finish sessionID,commandID,exitCode; session-close sessionID. Each value ends with newline. begin returns commandID newline or `excluded\n`. IDs are random16bytes rendered lowercasehex. An excluded command is not a persistence failure and has no finish event.

Foreground unlock derives key from the fixed validated vault salt/KDF tuple and sends only keyhex through private IPC. repair-key similarly carries a derived key and authenticates a truncated vault prefix without requiring strict unlock. Neither route accepts passwords in arguments or environment. See source for operation payload parsing; protocol is local and versioned, not upstream-compatible.

Encrypted model events are UTF-8 tab-delimited start/finish/delete/clear/retention/exclude records, with sensitive byte fields hex-encoded INSIDE authenticated encryption. See history-crypto-review.md for exact container framing. Public Terms JSONL import/export has string fields command_hex,cwd_hex,time,duration,exit; empty duration/exit denote incomplete. Import assigns fresh IDs and commits line by line: later errors may leave earlier records imported. No atomic-import claim.

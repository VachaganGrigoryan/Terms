#!/usr/bin/env python3
"""Process checks for both installed shells; no product dependency on Python."""

import os
import pathlib
import subprocess
import tempfile

project = pathlib.Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="terms-shell-test-") as temp:
    root = pathlib.Path(temp)
    target = root / "space ' $(touch SHOULD_NOT_EXIST)\n\n"
    target.mkdir()
    fake = root / "terms-fixture"
    fake.write_text(
        "#!/bin/sh\nif [ \"${TERMS_TEST_CANCEL-}\" = 1 ]; then exit 130; fi\ncase \"$1 $2\" in\n 'jump query'|'jump list') printf '%s\\n' \"$TERMS_TEST_HEX\";;\n 'jump add') exit 0;;\n 'search --files') printf 'sample\\0'; exit 2;;\n 'pick --read0') touch PICK_WAS_RUN; exit 0;;\n 'ls --probe') printf 'ls-ok\\n';;\n *) exit 2;;\nesac\n"
    )
    fake.chmod(0o700)
    env = dict(
        os.environ,
        TERMS_BIN=str(fake),
        TERMS_TEST_HEX=os.fsencode(target).hex(),
        TERMS_TEST_TARGET=str(target),
        TERMS_TEST_ERR=str(root / "enable.err"),
    )
    script = r"""
source "$TERMS_TEST_ADAPTER" || exit 10
tg() { echo user-tg; }
terms_enable 2>"$TERMS_TEST_ERR" || exit 11
[ "$(tl --probe)" = ls-ok ] || exit 24
[ "$(t ls --probe)" = ls-ok ] || exit 25
[ "$(tg)" = user-tg ] || exit 26
grep -q 'tg already exists' "$TERMS_TEST_ERR" || exit 27
for name in t tl ts tq tp th; do command -v "$name" >/dev/null 2>&1 || exit 30; done
_terms_decode 00 && exit 12
_terms_decode ff0 && exit 13
_terms_decode zz && exit 14
_terms_decode 610a || exit 15
[ "${#TERMS_DECODED}" = 2 ] || exit 16
tj not-a-real-directory || exit 17
[ "$PWD" = "$TERMS_TEST_TARGET" ] || exit 18
[ ! -e SHOULD_NOT_EXIST ] || exit 19
TERMS_TEST_CANCEL=1; export TERMS_TEST_CANCEL
before=$PWD
tji >/dev/null 2>&1
rc=$?
[ "$rc" = 130 ] || exit 20
[ "$PWD" = "$before" ] || exit 21
unset TERMS_TEST_CANCEL
_terms_insert >/dev/null 2>&1
rc=$?
[ "$rc" = 2 ] || exit 22
[ ! -e PICK_WAS_RUN ] || exit 23
terms_disable
for name in t tl ts tq tp th; do ! command -v "$name" >/dev/null 2>&1 || exit 28; done
[ "$(tg)" = user-tg ] || exit 29
"""
    for shell, adapter in [("/bin/bash", "terms.bash"), ("/bin/zsh", "terms.zsh")]:
        runenv = dict(env, TERMS_TEST_ADAPTER=str(project / "shell" / adapter))
        result = subprocess.run(
            [shell, "-c", script],
            env=runenv,
            cwd=root,
            capture_output=True,
            check=False,
        )
        assert result.returncode == 0, (
            shell,
            result.returncode,
            result.stdout,
            result.stderr,
        )
        print(
            shell
            + ": decode, newline path, injection, cancellation, short names passed"
        )

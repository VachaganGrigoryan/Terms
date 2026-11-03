"""Regression: hostile state FIFOs must not block before the lock timeout."""

import os
import pathlib
import subprocess
import tempfile

binary = os.environ.get("TERMS_BIN") or str(
    pathlib.Path(__file__).resolve().parents[1] / "target/release/terms"
)
for name, args in [("jump.db", ["list"]), ("jump.lock", ["add", "/tmp"])]:
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory) / "terms"
        root.mkdir(mode=0o700)
        os.mkfifo(root / name, 0o600)
        result = subprocess.run(
            [binary, "jump", *args],
            env=dict(os.environ, XDG_STATE_HOME=directory),
            capture_output=True,
            timeout=3,
            check=False,
        )
        assert result.returncode == 2, (name, result.returncode, result.stderr)
print("FIFO database and lock rejected promptly")

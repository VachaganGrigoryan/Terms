#!/usr/bin/env python3
"""History integration tests with a fixture; no real command collection enabled."""

import json
import os
import pathlib
import subprocess
import tempfile

project = pathlib.Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="terms-history-shell-") as temporary:
    root = pathlib.Path(temporary)
    fake = root / "terms-fixture"
    log = root / "requests"
    history = root / "native-history"
    fake.write_text("""#!/usr/bin/env python3
import os,sys,json
operation=sys.argv[2]
lines=sys.stdin.read().splitlines() if operation in ['session-open','session-close','begin','finish'] else []
with open(os.environ['TH_TEST_LOG'],'a') as out: out.write(json.dumps([operation,lines])+'\\n')
if operation=='session-open': print('ab')
elif operation=='begin': print('cd')
elif operation=='pick': print('7072696e74662073616665')
""")
    fake.chmod(0o700)
    env = dict(os.environ, TERMS_BIN=str(fake), TH_TEST_LOG=str(log))
    commands = (
        "\n".join(
            [
                f"HISTFILE={str(history)!r}; HISTSIZE=100; SAVEHIST=100",
                f"source {str(project / 'shell/history.zsh')!r}",
                "terms_history_enable",
                "terms_history_enable",
                "printf 'sensitive-marker\\n'",
                "printf '%s\\n' \\\n'multiline-marker'",
                "false",
                "print -- status-after-false:$?",
                " printf 'excluded-marker\\n'",
                "terms_history_disable",
                "[[ ! -v HISTFILE ]] || exit 88",
                "exit 0",
            ]
        )
        + "\n"
    )
    result = subprocess.run(
        ["/bin/zsh", "-f", "-i"],
        input=commands.encode(),
        env=env,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
    assert b"status-after-false:1" in result.stdout, result.stdout
    assert not history.exists() or b"sensitive-marker" not in history.read_bytes()
    events = [json.loads(line) for line in log.read_text().splitlines()]
    begins = [event[1] for event in events if event[0] == "begin"]
    decoded = [bytes.fromhex(fields[2]).decode() for fields in begins]
    assert sum("sensitive-marker" in command for command in decoded) == 1, decoded
    assert any(
        "multiline-marker" in command and "\n" in command for command in decoded
    ), decoded
    assert not any("excluded-marker" in command for command in decoded), decoded
    assert all(
        len(fields) == 3
        and fields[0] == "ab"
        and bytes.fromhex(fields[1]).startswith(b"/")
        for fields in begins
    )
    assert any(event[0] == "finish" and event[1][-1] == "1" for event in events), events
    assert events[-1][0] == "session-close", events[-1]
    print(
        "Zsh interactive capture, status, leading-space exclusion and no native replay passed"
    )
    version = (
        subprocess.run(
            ["/bin/bash", "-c", "echo ${BASH_VERSINFO[0]}.${BASH_VERSINFO[1]}"],
            capture_output=True,
            check=True,
        )
        .stdout.decode()
        .strip()
    )
    major, minor = (int(part) for part in version.split("."))
    if (major, minor) < (4, 4):
        bash = subprocess.run(
            [
                "/bin/bash",
                "-c",
                f"source {str(project / 'shell/history.bash')!r}; terms_history_enable",
            ],
            env=env,
            capture_output=True,
            check=False,
        )
        assert bash.returncode == 2 and b"Bash 4.4+" in bash.stderr
        print(
            f"Bash {version} automatic capture refusal passed; modern Bash unavailable"
        )
    else:
        # Capture on Bash 4.4+ is not exercised here; this must not be read as passing coverage of it.
        print(
            f"Bash {version}: refusal check not applicable; modern Bash capture NOT tested"
        )

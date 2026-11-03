import os
import pty
import select
import signal
import termios
import time

binary = os.environ.get("TERMS_BIN") or os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "target", "debug", "terms"
)


def run(key, expected, close_input=True):
    r, w = os.pipe()
    out_r, out_w = os.pipe()
    pid, master = pty.fork()
    if pid == 0:
        os.dup2(r, 0)
        os.dup2(out_w, 1)
        os.close(w)
        os.close(out_r)
        os.execv(binary, [binary, "pick", "--read0", "--print0"])
    os.close(r)
    os.close(out_w)
    saved = termios.tcgetattr(master)
    os.write(w, b"alpha\0be\nta\0")
    if close_input:
        os.close(w)
    time.sleep(0.15)
    os.write(master, key)
    deadline = time.time() + 4
    status = None
    screen = b""
    while time.time() < deadline:
        ready, _, _ = select.select([master], [], [], 0.05)
        if ready:
            try:
                screen += os.read(master, 65536)
            except OSError:
                pass
        got, status = os.waitpid(pid, os.WNOHANG)
        if got:
            break
    else:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
        raise AssertionError("hung")
    result = os.read(out_r, 65536)
    assert os.waitstatus_to_exitcode(status) == expected, (status, result, screen)
    assert termios.tcgetattr(master) == saved, "terminal settings leaked"
    os.close(master)
    os.close(out_r)
    if not close_input:
        os.close(w)
    return result


assert run(b"\r", 0) == b"alpha\0"
assert run(b"\x03", 130, False) == b""
assert run(b"\x1b", 130, False) == b""
print(
    "PTY acceptance, stalled-producer Ctrl-C/Escape, byte output and termios restoration passed"
)

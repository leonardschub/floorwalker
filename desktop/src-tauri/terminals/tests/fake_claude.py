"""A stand-in for `claude attach <id>` (the spec's "fake attach"), driven through files.

Run as `python3 fake_claude.py --dir <dir> attach <id>`, under a PTY. In `<dir>`:

- `log` gets a line per event: `start <id> <pid>`, `hangup <id> <pid>`, `exit <id> <pid>`.
- `<id>.env` gets the environment it was started with, one `KEY=value` per line.
- `<id>.replay`, when present, is written to the terminal on start: captured attach output
  (`fixtures/session.bytes` is a real attach's).
- `<id>.input` gets every byte typed into it.
- `<id>.cmd` is read and emptied every 20 ms; each line is a command:
  - `write <path>`: writes that file's bytes to the terminal;
  - `agents <path>`: becomes agent view as the real attach does: retitles itself `claude
    agents` (its `/proc/<pid>/cmdline` reads `claude\\0agents\\0…`, by re-executing itself
    under that argv, same pid) and then writes `<path>` (captured agent view output);
  - `exit`: leaves the alternate screen and exits with code 0, as Ctrl+Z makes the real
    attach do.

A size change (SIGWINCH) logs `winch <id> <cols>x<rows>` and, as Claude Code does when its
own size changes, clears the screen and draws `<id>.replay` again.

A hang-up (SIGHUP) ends it, as it ends the real one.
"""

import fcntl
import os
import select
import struct
import signal
import sys
import termios
import time
import tty


def main():
    args = sys.argv[1:]
    directory = args[args.index("--dir") + 1]
    if os.path.basename(sys.argv[0]) == "agents":
        # Re-executed as agent view: Python runs the script `agents`, so its own argv is
        # `agents --dir <dir> <id> <path>`.
        short_id, then = args[2], args[3]
    else:
        short_id, then = args[args.index("attach") + 1], None
    pid = os.getpid()

    def log(event, detail=None):
        with open(os.path.join(directory, "log"), "a") as file:
            line = f"{event} {short_id}" + (f" {detail}" if detail else "")
            file.write(f"{line} {pid}\n")

    def hang_up(_signal, _frame):
        log("hangup")
        os._exit(0)

    signal.signal(signal.SIGHUP, hang_up)
    out = sys.stdout.buffer
    resized = []

    def size():
        rows, cols, _, _ = struct.unpack("HHHH", fcntl.ioctl(0, termios.TIOCGWINSZ, b"\0" * 8))
        return cols, rows

    def winch(_signal, _frame):
        cols, rows = size()
        log("winch", f"{cols}x{rows}")
        resized.append((cols, rows))

    if os.isatty(0):
        signal.signal(signal.SIGWINCH, winch)

    def write_file(path):
        with open(path, "rb") as file:
            out.write(file.read())
        out.flush()

    if then is None:
        log("start")
        with open(os.path.join(directory, f"{short_id}.env"), "w") as file:
            for key, value in os.environ.items():
                file.write(f"{key}={value}\n")
        if os.isatty(0):
            tty.setraw(0, termios.TCSANOW)
        replay = os.path.join(directory, f"{short_id}.replay")
        if os.path.exists(replay):
            write_file(replay)
    else:
        write_file(then)

    commands = os.path.join(directory, f"{short_id}.cmd")
    while True:
        ready, _, _ = select.select([0], [], [], 0.02)
        if resized:
            del resized[:]
            replay = os.path.join(directory, f"{short_id}.replay")
            out.write(b"\x1b[2J\x1b[H")
            out.flush()
            if os.path.exists(replay):
                write_file(replay)
        if ready:
            try:
                typed = os.read(0, 4096)
            except OSError:
                # The terminal was hung up; the read can win the race with SIGHUP.
                hang_up(None, None)
            if not typed:
                log("exit")
                return
            with open(os.path.join(directory, f"{short_id}.input"), "ab") as file:
                file.write(typed)
        if not os.path.exists(commands):
            continue
        with open(commands, "r+") as file:
            lines = file.read().splitlines()
            file.truncate(0)
        for line in lines:
            command, _, argument = line.partition(" ")
            if command == "write":
                write_file(argument)
            elif command == "agents":
                # A file named `agents` beside the log, so the re-executed Python finds
                # its script under the argv the real attach shows.
                link = os.path.join(directory, "agents")
                if not os.path.exists(link):
                    os.symlink(os.path.abspath(__file__), link)
                os.chdir(directory)
                os.execv(sys.executable, ["claude", "agents", "--dir", directory, short_id, argument])
            elif command == "exit":
                # Leaving the alternate screen first, as the real attach does.
                out.write(b"\x1b[?1049l")
                out.flush()
                log("exit")
                sys.exit(0)


if __name__ == "__main__":
    main()

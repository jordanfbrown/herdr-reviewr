#!/usr/bin/env python3
"""Prove `e` really suspends the pane, runs the editor, and comes back.

Unit tests stop at the argv. Everything after it is terminal state: leaving the alternate
screen, releasing raw mode, and rebuilding both on return. Only a real PTY shows that, so this
drives the real binary with a scripted editor and checks the bytes on the wire.

    python3 scripts/smoke_edit_file.py --binary target/release/herdr-reviewr

Exits 0 when every check passes, 1 with the failing check named otherwise.
"""

import argparse
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time

ROWS, COLS = 40, 120
ALT_ENTER = b"\x1b[?1049h"
ALT_LEAVE = b"\x1b[?1049l"
EDITED_LINE = "EDITED-BY-THE-SCRIPTED-EDITOR"


def sh(cwd, *args):
    subprocess.run(args, cwd=cwd, check=True, capture_output=True)


def make_repo(root):
    """A repo with one committed file and one uncommitted edit, so `Changes` has a diff."""
    sh(root, "git", "init", "-q")
    sh(root, "git", "config", "user.email", "smoke@example.com")
    sh(root, "git", "config", "user.name", "Smoke")
    path = os.path.join(root, "a.rs")
    with open(path, "w") as f:
        f.write("one\ntwo\nthree\nfour\n")
    sh(root, "git", "add", "-A")
    sh(root, "git", "commit", "-qm", "init")
    with open(path, "w") as f:
        f.write("one\nTWO\nthree\nfour\nfive\n")
    return path


def make_editor(bindir, argv_log, name):
    """A scripted `$EDITOR` under a real editor's name, so its dialect resolves.

    Records its argv, writes to the file, and prints to the terminal. It lives outside the
    repository, or reviewr would list it as an untracked change and open it instead.
    """
    os.makedirs(bindir, exist_ok=True)
    script = os.path.join(bindir, name)
    with open(script, "w") as f:
        f.write(
            "#!/bin/sh\n"
            f'printf "%s\\n" "$@" > {argv_log}\n'
            # Proof the editor owns a real terminal: this reaches the screen only if reviewr
            # actually left the alternate screen.
            'printf "FAKE-EDITOR-IS-ON-SCREEN\\n"\n'
            # The last argument carries the path in every dialect, bare or `path:line`.
            'for a in "$@"; do last="$a"; done\n'
            'printf "%s\\n" "${last%%:*}" > /dev/null\n'
            f'printf "{EDITED_LINE}\\n" >> "${{last%%:*}}"\n'
            "exit 0\n"
        )
    os.chmod(script, 0o755)
    return script


class Session:
    def __init__(self, binary, repo, editor):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        env = {**os.environ, "TERM": "xterm-256color"}
        env.pop("VISUAL", None)
        env.pop("EDITOR", None)
        if editor:
            env["EDITOR"] = editor
        env.pop("HERDR_PLUGIN_CONFIG_DIR", None)  # standalone: no plugin config reads
        self.proc = subprocess.Popen(
            [binary, repo, "--poll", "600000"],
            stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True,
        )
        os.close(slave)
        self.seen = b""

    def drain(self, quiet=0.5, timeout=20.0):
        deadline = time.perf_counter() + timeout
        got = b""
        while time.perf_counter() < deadline:
            r, _, _ = select.select([self.master], [], [], quiet if got else 1.0)
            if not r:
                if got:
                    break
                continue
            try:
                data = os.read(self.master, 65536)
            except OSError:
                break
            if not data:
                break
            got += data
        self.seen += got
        return got

    def press(self, key):
        os.write(self.master, key.encode())
        return self.drain()

    def close(self):
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=5)
        os.close(self.master)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", default="target/release/herdr-reviewr")
    args = ap.parse_args()
    binary = os.path.abspath(args.binary)
    if not os.path.exists(binary):
        sys.exit(f"no binary at {binary}: cargo build --release first")

    failures = []

    def check(name, ok, detail=""):
        print(f"{'ok  ' if ok else 'FAIL'}  {name}{'  ' + detail if detail and not ok else ''}")
        if not ok:
            failures.append(name)

    with tempfile.TemporaryDirectory() as home:
        root = os.path.join(home, "repo")
        bindir = os.path.join(home, "bin")
        os.makedirs(root)
        file_path = make_repo(root)
        argv_log = os.path.join(home, "argv.txt")
        editor = make_editor(bindir, argv_log, "vim")

        s = Session(binary, root, editor)
        s.drain()  # startup paint
        check("the pane starts on the alternate screen", ALT_ENTER in s.seen)

        before = len(s.seen)
        out = s.press("e")
        after = s.seen[before:]

        check("`e` leaves the alternate screen", ALT_LEAVE in after)
        check("the editor's own output reaches the terminal", b"FAKE-EDITOR-IS-ON-SCREEN" in after)
        check("the pane re-enters the alternate screen", ALT_ENTER in after)
        check(
            "the alternate screen is left before it is re-entered",
            ALT_LEAVE in after
            and ALT_ENTER in after
            and after.index(ALT_LEAVE) < after.rindex(ALT_ENTER),
        )

        argv = []
        if os.path.exists(argv_log):
            with open(argv_log) as f:
                argv = [line.rstrip("\n") for line in f]
        check("the editor is handed an absolute path", bool(argv) and argv[-1].startswith("/"),
              f"argv={argv}")
        # `vim` resolves to the `+LINE` dialect, so the line arrives in its own form.
        check("and the line under the cursor, in the vi dialect",
              bool(argv) and argv[0].startswith("+") and argv[0][1:].isdigit(),
              f"argv={argv}")

        with open(file_path) as f:
            body = f.read()
        check("the editor's write lands in the worktree", EDITED_LINE in body)

        out = s.press("r")
        check("reviewr repaints after the refresh", len(out) > 0)
        check("the status names the edited file", b"edited" in s.seen)
        # The resume must repaint the whole pane, or the editor's leftovers stay on screen.
        check("the resumed frame repaints the whole pane",
              ALT_ENTER in after
              and b"Changes" in after
              and after.rindex(b"Changes") > after.index(ALT_ENTER),
              "the pane did not redraw after re-entering the alternate screen")

        s.press("q")
        check("`q` still quits", s.proc.wait(timeout=10) == 0)
        s.close()

        # A graphical editor takes a different dialect and must be made to block, or the pane
        # repaints before the reviewer has typed anything.
        gui_log = os.path.join(home, "argv-gui.txt")
        gui = make_editor(bindir, gui_log, "code")
        s = Session(binary, root, gui)
        s.drain()
        s.press("e")
        gui_argv = []
        if os.path.exists(gui_log):
            with open(gui_log) as f:
                gui_argv = [line.rstrip("\n") for line in f]
        check("a graphical editor is told to wait", "--wait" in gui_argv, f"argv={gui_argv}")
        check("the pane comes back painted", b"Changes" in s.seen)
        check("and takes its line as --goto path:line",
              "-g" in gui_argv and bool(gui_argv) and ":" in gui_argv[-1],
              f"argv={gui_argv}")
        s.press("q")
        s.close()

        # Every failure path has to reach the reviewer on the frame it happened, not on the
        # next keypress. The loop draws only after an event arrives, so the run has to repaint.
        for label, ed, needle in [
            ("no editor set", None, b"set `editor`"),
            ("a missing editor binary", "/nonexistent/nope", b"editor failed"),
            ("an editor that exits nonzero", "/usr/bin/false", b"editor exited"),
        ]:
            s = Session(binary, root, ed)
            s.drain()
            mark = len(s.seen)
            s.press("e")
            check(f"{label} says so on the press", needle in s.seen[mark:])
            s.press("q")
            s.close()

    if failures:
        print(f"\n{len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

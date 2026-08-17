#!/usr/bin/env python3
"""Receive a badge export over USB serial.

    python3 tools/recv-export.py            # watch every /dev/ttyACM*, save what arrives
    python3 tools/recv-export.py /dev/ttyACM1 -o photo.txt

Use this rather than `cat`. The port comes up with `min = 0; time = 0`, which makes
read() return zero bytes immediately - `cat` reads that as end-of-file and exits at
once, which looks exactly like the badge having sent nothing. This sets the line to
raw with a blocking read first.

It also watches every ACM port at once by default, because the badge can present
more than one and the export is not necessarily on the lowest-numbered.
"""

import argparse
import glob
import os
import select
import sys
import termios
import time


def open_raw(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    iflag, oflag, cflag, lflag, ispeed, ospeed, cc = attrs
    # raw: no translation, no echo, no signal characters
    iflag &= ~(termios.IXON | termios.IXOFF | termios.ICRNL | termios.INLCR | termios.IGNCR)
    oflag &= ~termios.OPOST
    lflag &= ~(termios.ICANON | termios.ECHO | termios.ECHOE | termios.ISIG)
    # CLOCAL: do not wait on carrier. CREAD: actually receive.
    cflag |= termios.CLOCAL | termios.CREAD
    cc[termios.VMIN] = 1
    cc[termios.VTIME] = 0
    termios.tcsetattr(fd, termios.TCSANOW, [iflag, oflag, cflag, lflag, ispeed, ospeed, cc])
    return fd


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ports", nargs="*", help="serial devices; default is every /dev/ttyACM*")
    ap.add_argument("-o", "--out", default="export.txt")
    ap.add_argument("--idle", type=float, default=2.0, help="stop after this many quiet seconds")
    ap.add_argument("--wait", type=float, default=300.0,
                    help="give up if no export data arrives at all. Generous on purpose: "
                         "this is the time you have to walk the badge menus.")
    args = ap.parse_args()

    ports = args.ports or sorted(glob.glob("/dev/ttyACM*"))
    if not ports:
        sys.exit("no /dev/ttyACM* found - is the badge plugged in?")

    fds = {}
    for p in ports:
        try:
            fds[open_raw(p)] = p
        except OSError as e:
            print(f"  {p}: cannot open ({e})")
    if not fds:
        sys.exit("could not open any port")

    print(f"listening on {', '.join(fds.values())}")
    print("now run the export on the badge: photos -> more -> export -> yes")

    # The badge's log shares these interfaces with the export. Log traffic must not start
    # the idle countdown, or simply walking the menus to reach the export ends the capture
    # before it begins - which is exactly what it did.
    def is_log(line):
        return line.startswith(
            (b"INFO:", b"WARN:", b"ERR:", b"ERROR:", b"DEBUG:", b"TRACE:")
        ) or b"(src/" in line or b"(services/" in line

    data = bytearray()
    source = None
    started = time.time()
    last = None          # last time PAYLOAD arrived, not last time anything arrived
    pending = bytearray()
    while True:
        ready, _, _ = select.select(list(fds), [], [], 0.25)
        for fd in ready:
            chunk = os.read(fd, 4096)
            if not chunk:
                continue
            pending += chunk
            # Judge whole lines only: a log line has to be complete to be recognised.
            parts = pending.split(b"\n")
            pending = bytearray(parts.pop())  # trailing fragment, not yet a line
            for line in parts:
                if is_log(line.rstrip(b"\r")):
                    continue
                if source is None:
                    source = fds[fd]
                    print(f"receiving on {source}")
                data += line + b"\n"
                last = time.time()
        if last and time.time() - last > args.idle:
            break
        if not last and time.time() - started > args.wait:
            sys.exit("nothing but log traffic arrived - did the export run?")

    for fd in fds:
        os.close(fd)
    with open(args.out, "wb") as fh:
        fh.write(data)
    print(f"{len(data)} bytes from {source} -> {args.out}")
    print(f"now: python3 {os.path.dirname(__file__)}/show-export.py {args.out}")


if __name__ == "__main__":
    main()

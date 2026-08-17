#!/usr/bin/env python3
"""Follow the badge's log stream until you stop it.

    python3 tools/watch-badge.py                 # print and save to badge.log
    python3 tools/watch-badge.py -o crash.log    # save somewhere else

Written for catching something intermittent - a panic that only shows up after a
few tries. It differs from recv-export.py in the ways that matter for that:

  * it never stops on its own. recv-export gives up after a couple of quiet
    seconds, which is right for one export and useless for a stakeout.
  * it reopens ports as they come and go, so a badge that crashes, reboots or
    gets unplugged is picked up again without restarting the capture.
  * every line is timestamped and flushed to disk as it arrives, so whatever is
    on screen when it dies is already saved.

Lines that look like a panic are marked with >>> so they can be found afterwards
with: grep -n '>>>' badge.log
"""

import argparse
import glob
import os
import select
import sys
import termios
import time

PANIC = ("panic", "Guru", "guru", "couldn't return memory", "ProcessNotFound",
         "unwrap", "assertion", "PANIC", "halt")


def open_raw(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOCTTY | os.O_NONBLOCK)
    iflag, oflag, cflag, lflag, ispeed, ospeed, cc = termios.tcgetattr(fd)
    iflag &= ~(termios.IXON | termios.IXOFF | termios.ICRNL | termios.INLCR | termios.IGNCR)
    oflag &= ~termios.OPOST
    lflag &= ~(termios.ICANON | termios.ECHO | termios.ECHOE | termios.ISIG)
    cflag |= termios.CLOCAL | termios.CREAD
    cc[termios.VMIN] = 1
    cc[termios.VTIME] = 0
    termios.tcsetattr(fd, termios.TCSANOW, [iflag, oflag, cflag, lflag, ispeed, ospeed, cc])
    return fd


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-o", "--out", default="badge.log")
    ap.add_argument("--quiet", action="store_true", help="write to the file only")
    ap.add_argument("--port", help="listen to this port only, e.g. /dev/ttyACM2. Several "
                                   "interfaces carry the same log; narrow to one when a "
                                   "trace has to be counted exactly.")
    args = ap.parse_args()

    log = open(args.out, "a", buffering=1)
    log.write(f"\n=== capture started {time.strftime('%Y-%m-%d %H:%M:%S')} ===\n")
    print(f"following every /dev/ttyACM*, appending to {args.out}. Ctrl-C to stop.")
    print("reproduce the fault now; whatever is captured is already on disk.\n")

    fds = {}          # fd -> path
    partial = {}      # fd -> bytes not yet ending in a newline
    last_scan = 0.0
    # The badge presents several CDC interfaces and more than one carries the same log, so
    # listening to all of them records some lines twice. That is not harmless - it made a
    # keystroke trace look as though characters were being sent twice, and cost a round of
    # chasing a duplicate that was never there. Each line is therefore tagged with the port
    # it arrived on, and --port narrows the capture to one when a trace has to be exact.
    #
    # Dropping repeats by content would be wrong: "https" really does log 't' twice in a row,
    # and a deduplicating capture would quietly delete the evidence it exists to collect.

    try:
        while True:
            # pick up ports as they appear, and after a reboot re-enumerates them
            if time.time() - last_scan > 1.0:
                last_scan = time.time()

                # Drop fds whose device node has been replaced underneath us. A badge that
                # reboots re-enumerates: the old fd stays open and valid-looking but is
                # attached to a device that no longer exists, and select() never reports it
                # readable - so without this check the read path never notices, the path
                # still counts as open, and the capture goes silently deaf. That is exactly
                # what it happened to do while waiting for a panic.
                for fd, path in list(fds.items()):
                    try:
                        fresh = os.stat(path)
                        stale = os.fstat(fd).st_ino != fresh.st_ino
                    except OSError:
                        stale = True
                    if stale:
                        fds.pop(fd, None)
                        partial.pop(fd, None)
                        try:
                            os.close(fd)
                        except OSError:
                            pass
                        print(f"--- {path} was replaced (badge re-enumerated); reopening ---")

                for path in sorted([args.port] if args.port else glob.glob("/dev/ttyACM*")):
                    if path not in fds.values():
                        try:
                            fd = open_raw(path)
                        except OSError:
                            continue
                        fds[fd] = path
                        partial[fd] = b""
                        print(f"--- opened {path} ---")

            if not fds:
                time.sleep(0.25)
                continue

            ready, _, _ = select.select(list(fds), [], [], 0.25)
            for fd in ready:
                try:
                    chunk = os.read(fd, 4096)
                except OSError:
                    chunk = b""
                if not chunk:
                    # the badge went away - drop it and let the scan pick it up again
                    path = fds.pop(fd)
                    partial.pop(fd, None)
                    os.close(fd)
                    print(f"--- {path} closed (badge reset or unplugged) ---")
                    continue

                partial[fd] += chunk
                *lines, partial[fd] = partial[fd].split(b"\n")
                for raw in lines:
                    text = raw.decode("utf-8", "replace").rstrip("\r")
                    if not text:
                        continue
                    mark = ">>> " if any(p in text for p in PANIC) else "    "
                    # milliseconds, because the things worth timing here - a keystroke, a
                    # whole type-out - all happen inside one second
                    now = time.time()
                    stamp = time.strftime("%H:%M:%S", time.localtime(now)) + f".{int(now % 1 * 1000):03d}"
                    tag = fds[fd][-1]  # which interface this arrived on
                    line = f"{stamp} [{tag}] {mark}{text}"
                    log.write(line + "\n")
                    if not args.quiet:
                        print(line)
    except KeyboardInterrupt:
        print(f"\nstopped. {args.out} holds the capture.")
        print(f"find the interesting part with:  grep -n '>>>' {args.out}")
    finally:
        for fd in list(fds):
            os.close(fd)
        log.close()


if __name__ == "__main__":
    main()

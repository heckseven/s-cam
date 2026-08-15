#!/usr/bin/env bash
# flash-queue.sh - flash a queue of staged badge builds back to back in one sitting.
#
# Each staged build is a directory containing:
#   loader.uf2  xous.uf2  swap.uf2  SHA256SUMS   (required)
#   LABEL       one line, what this build is                      (optional)
#   EXPECT      what to look for, and what counts as pass         (optional)
#
# Builds are flashed in lexical order, so name them 01-..., 02-..., 99-rollback.
# Between builds the script waits for the badge to re-enter bootloader mode, so
# the loop is: flash -> you boot and observe -> hold button, replug -> next flash.
#
# Usage: flash-queue.sh <queue-dir> [start-index]
# Results are appended to <queue-dir>/RESULTS.log

set -uo pipefail
QUEUE="${1:?usage: flash-queue.sh <queue-dir> [start-index]}"
START="${2:-1}"
LOG="$QUEUE/RESULTS.log"

[ -d "$QUEUE" ] || { echo "no such queue dir: $QUEUE" >&2; exit 1; }
mapfile -t BUILDS < <(find "$QUEUE" -mindepth 1 -maxdepth 1 -type d | sort)
[ "${#BUILDS[@]}" -gt 0 ] || { echo "queue is empty: $QUEUE" >&2; exit 1; }

# --- preflight: verify every build BEFORE asking the user to sit down ---
echo "=== preflight: verifying ${#BUILDS[@]} staged build(s) ==="
fail=0
for b in "${BUILDS[@]}"; do
  name=$(basename "$b")
  for f in loader.uf2 xous.uf2 swap.uf2 SHA256SUMS; do
    [ -f "$b/$f" ] || { echo "  $name: MISSING $f"; fail=1; }
  done
  [ -f "$b/SHA256SUMS" ] && { ( cd "$b" && sha256sum -c SHA256SUMS >/dev/null 2>&1 ) \
      && echo "  $name: checksums OK" || { echo "  $name: CHECKSUM MISMATCH"; fail=1; }; }
done
# The last build in a queue must be the known-good rollback, so that no session
# can end with the badge in an unknown state. Note we deliberately do NOT attempt
# a post-write read-back: the BAOCHIP vdisk is virtual and reads back empty, so a
# checksum after dd is satisfied by page cache and proves nothing.
last=$(basename "${BUILDS[-1]}")
if ! grep -qi 'rollback\|known-good' <<<"$last" && [ ! -f "${BUILDS[-1]}/ROLLBACK" ]; then
  echo "  WARNING: last queue entry '$last' is not marked as a rollback."
  echo "  End every queue with the known-good triple (name it *rollback* or add a ROLLBACK marker file)"
  echo "  so a failed session never leaves the badge unbootable."
  fail=1
fi
[ "$fail" -eq 0 ] || { echo "preflight failed - fix before starting a session" >&2; exit 1; }
echo

wait_for_badge() {
  local dev=""
  for _ in $(seq 1 900); do
    dev=$(lsblk -nro NAME,LABEL 2>/dev/null | awk '$2=="BAOCHIP"{print "/dev/"$1}' | head -1)
    [ -n "$dev" ] && { echo "$dev"; return 0; }
    sleep 2
  done
  return 1
}

flash_one() {
  local src="$1" dev="$2" dst
  dst=$(findmnt -n -o TARGET "$dev" 2>/dev/null || true)
  if [ -z "$dst" ]; then
    for _ in 1 2 3 4 5; do udisksctl mount -b "$dev" >/dev/null 2>&1 && break; sleep 2; done
    dst=$(findmnt -n -o TARGET "$dev" 2>/dev/null || true)
  fi
  [ -n "$dst" ] || { echo "  ERROR: could not mount $dev (badge may have left bootloader mode)"; return 1; }
  mountpoint -q "$dst" || { echo "  ERROR: '$dst' is not a mountpoint"; return 1; }
  for f in loader.uf2 xous.uf2 swap.uf2; do
    printf "  writing %-12s ... " "$f"
    dd if="$src/$f" of="$dst/$f" bs=1M conv=fsync status=none || { echo "FAILED"; return 1; }
    echo "ok"
  done
  sync; sync
  udisksctl unmount -b "$dev" >/dev/null 2>&1 || umount "$dst" >/dev/null 2>&1 || true
  findmnt "$dev" >/dev/null 2>&1 && { echo "  ERROR: still mounted - do NOT boot"; return 1; }
  return 0
}

echo "=== session $(date -u +%Y-%m-%dT%H:%M:%SZ)  queue=$(basename "$QUEUE")  builds=${#BUILDS[@]} ===" >> "$LOG"
i=0
for b in "${BUILDS[@]}"; do
  i=$((i+1))
  [ "$i" -lt "$START" ] && continue
  name=$(basename "$b")
  label=$([ -f "$b/LABEL" ] && head -1 "$b/LABEL" || echo "(no label)")
  echo "────────────────────────────────────────────────────────"
  echo "[$i/${#BUILDS[@]}] $name"
  echo "  $label"
  [ -f "$b/EXPECT" ] && { echo "  expect:"; sed 's/^/    /' "$b/EXPECT"; }
  echo
  echo "  >>> hold any button and plug the badge in (waiting up to 30 min)..."
  dev=$(wait_for_badge) || { echo "  badge never appeared - stopping"; echo "$name: ABORTED (no badge)" >> "$LOG"; exit 1; }
  echo "  found $dev"; sleep 2
  if flash_one "$b" "$dev"; then
    echo "  FLASH OK - press any button on the badge to boot and observe."
    echo "$name: WRITTEN (verdict pending) - $label" >> "$LOG"
    echo "    ^ record the observed result with: flash-queue-verdict.sh \"$QUEUE\" \"$name\" pass|fail \"notes\"" >> "$LOG"
  else
    echo "  FLASH FAILED - stopping so you can investigate."
    echo "$name: FLASH FAILED" >> "$LOG"
    exit 1
  fi
  echo
done
echo "────────────────────────────────────────────────────────"
echo "queue complete. results in $LOG"

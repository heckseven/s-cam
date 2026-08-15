#!/usr/bin/env bash
# Record what actually happened when a staged build was booted.
#
# flash-queue.sh can only report that bytes were written. Whether the badge then
# booted is an observation only a human can make, and a session's entire value is
# in those observations - so they have to land in the log, not in someone's memory.
#
# Usage: flash-queue-verdict.sh <queue-dir> <build-name> pass|fail|unobserved [notes]
#
# "unobserved" is a distinct verdict on purpose. A build that was flashed but never
# booted is NOT a failure, and recording it as one poisons the record - a later reader
# would conclude the image is bad when nobody ever looked at it.
set -uo pipefail
Q="${1:?usage: flash-queue-verdict.sh <queue-dir> <build-name> pass|fail|unobserved [notes]}"
NAME="${2:?build name}"
VERDICT="${3:?pass, fail, or unobserved}"
NOTES="${4:-}"
case "$VERDICT" in pass|fail|unobserved) ;; *) echo "verdict must be 'pass', 'fail', or 'unobserved'" >&2; exit 1;; esac
[ -d "$Q/$NAME" ] || { echo "no such build in queue: $Q/$NAME" >&2; exit 1; }
printf '%s: VERDICT=%s %s%s\n' "$NAME" "$VERDICT" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  "${NOTES:+ - $NOTES}" >> "$Q/RESULTS.log"
echo "recorded: $NAME = $VERDICT"

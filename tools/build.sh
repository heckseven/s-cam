#!/usr/bin/env bash
#
# Build a flashable S-CAM firmware set from source.
#
# Use this rather than calling `cargo xtask baosec-lite` directly. xtask bundles whatever app
# ELFs it is pointed at; it does NOT build them. Running xtask alone therefore happily
# packages a stale dc34-vault and produces a swap.uf2 that silently omits your changes -
# it prints a full, successful-looking build the whole way through.
#
# Output: loader.uf2, xous.uf2, swap.uf2 under xous-core's release dir.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
vault=$(dirname -- "$here")
root=$(dirname -- "$vault")
console="$root/s-cam-console"
core="$root/xous-core"
target=riscv32imac-unknown-xous-elf

for d in "$console" "$core"; do
    [ -d "$d" ] || { echo "missing sibling checkout: $d" >&2; exit 1; }
done

echo "===== Building console ====="
(
    cd "$console"
    cargo build --release --target "$target" \
        --features board-baosec --features oem-baosec-lite \
        --features bao1x --features utralib/bao1x
)

echo "===== Building vault ====="
(
    cd "$vault"
    cargo build --release --target "$target" --features board-baosec
)

vault_elf="$vault/target/$target/release/dc34-vault"
console_elf="$console/target/$target/release/dc34-console"

# Guard before bundling, not after. Past the page limit the badge hangs partway through its
# boot progress bar with no error and has to be reflashed to recover, so a build that cannot
# boot should never reach the point where it looks flashable.
echo "===== Checking size ====="
python3 "$vault/check-app-size.py" "$vault_elf"

# Source-level guards. These are cheap and catch two classes of silently-dead code that have
# each shipped to hardware before: a menu entry wired to a handler that cannot receive it, and
# a camera call site that ignores what the camera reported.
echo "===== Checking wiring ====="
# Every check-*.py in this directory, found by glob rather than listed by hand. The list was
# hand-maintained and had drifted: five checkers - photo-buffer, button-labels,
# key-consumption, list-wiring, modal-exits - existed in the repo and were never run by a
# build, so they only fired when someone remembered them. A guard that is not wired in is
# not a guard. Adding a checker now enforces it automatically.
#
# check-dead-code.py is excluded here and run below instead: it shells out to cargo and must
# run from the vault checkout, not from tools/.
for check in "$here"/check-*.py; do
    [ "$(basename "$check")" = "check-dead-code.py" ] && continue
    python3 "$check"
done
# Runs cargo again, but the vault was just built so this is a cache hit and costs seconds.
(cd "$vault" && python3 "$here/check-dead-code.py")

echo "===== Bundling ====="
(
    cd "$core"
    cargo xtask baosec-lite "$console_elf~flash" "$vault_elf" \
        --no-timestamp --feature usb --kernel-feature debug-proc --no-verify
)

out="$core/target/$target/release"
echo
echo "Built from:"
# Timestamps are the check that the bundle really contains this build - compare them against
# your last edit if anything looks unchanged on the badge.
stat -c '  %y  %n' "$vault_elf" "$console_elf" "$out/swap.uf2" "$out/xous.uf2" "$out/loader.uf2"
echo
sha256sum "$out/loader.uf2" "$out/xous.uf2" "$out/swap.uf2"

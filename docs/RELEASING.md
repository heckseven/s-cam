# Cutting a release

Users update a badge by copying three `.uf2` files onto it. A release exists to hand them
those three files, as a matched set, from a build that has been run on real hardware.

## The one rule

**Never release a build that has not booted on a badge.**

Not "it compiled". Not "the checkers passed". Not "it fits". On 2026-08-18 a build passed all
fourteen checkers, measured 277 of 307 pages, and boot-looped the badge anyway - the cause was
code layout, and no host-side check catches it. See the layout hazard in
[DECISIONS.md](../DECISIONS.md).

Everywhere else a bad build costs a rebuild. Here it costs the user a badge that loops until
they find their way back into the bootloader.

## Steps

1. Build, which runs the size and wiring guards:

   ```shell
   ./tools/build.sh
   ```

2. Stage it and flash it to a badge, with a known-good rollback behind it. `flash-queue.sh`
   refuses a queue without one:

   ```shell
   ./tools/flash-queue.sh flash-queue/<session>
   ./tools/flash-queue-verdict.sh flash-queue/<session> <build> pass "booted, checked X and Y"
   ```

3. Only once it boots, publish the three files plus their checksums:

   ```shell
   cd ../xous-core/target/riscv32imac-unknown-xous-elf/release
   sha256sum loader.uf2 xous.uf2 swap.uf2 > SHA256SUMS
   gh release create vX.Y.Z --repo heckseven/s-cam \
       loader.uf2 xous.uf2 swap.uf2 SHA256SUMS
   ```

   `--draft` first is a good habit: it lets you read the notes as a user would before anyone
   can download them.

## What the notes must say

- **All three files, together.** They are built as a set and are not independent. Copying a
  new `swap.uf2` over an old `xous.uf2` is a good way to produce a badge that does not boot.
- **Flashing is a one-way door on a stock badge.** It wipes `k0` and enables developer mode
  permanently. A badge that has already run custom firmware is already past this.
- **How to recover.** Hold any button while plugging in to re-enter the bootloader. This works
  even when the firmware does not boot, which is what makes a failed flash survivable.
- **Unmount before unplugging** under Linux, or the last sector may not be written.

## What is deliberately not automated

There is no CI. A build needs all four repos side by side *and* the custom
`riscv32imac-unknown-xous-elf` toolchain from `cargo xtask install-toolkit`, and the release
gate is a human confirming a badge booted - which CI cannot do. Automating the build would
only move the easy half.

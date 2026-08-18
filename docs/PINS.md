# Dependency Pins

- dc34-vault: 7954e6200df67580795b12602e1a7235ed434ca6
- dc34-api: 617f0f3dff3cea1e9421d766b19664f5bec9a54b
- dc34-console: bf64e03f019532cca5055fcdbe51977d572e3630
- xous-core: 616bf65f6e379165464f50b1e79ec42aff77a683 (pinned by dc34-vault Cargo.lock, NOT HEAD)

## Reproduction

Run each block from a fresh working directory (each `cd` is relative to that base):

```
git clone https://github.com/bunnie/dc34-vault
(cd dc34-vault && git checkout 7954e6200df67580795b12602e1a7235ed434ca6)

git clone https://github.com/bunnie/dc34-api
(cd dc34-api && git checkout 617f0f3dff3cea1e9421d766b19664f5bec9a54b)

git clone https://github.com/bunnie/dc34-console
(cd dc34-console && git checkout bf64e03f019532cca5055fcdbe51977d572e3630)

git clone https://github.com/betrusted-io/xous-core
(cd xous-core && git checkout 616bf65f6e379165464f50b1e79ec42aff77a683)
```

Then install the toolchain and build (**requires user authorization** in a trusted shell; see HARDWARE.md Build Status):

```
(cd xous-core && cargo xtask install-toolkit && cargo xtask baosec-lite)
```

## Notes

- xous-core pin `616bf65f` is confirmed present: `git cat-file -t 616bf65f6e379165464f50b1e79ec42aff77a683` returns `commit` in a clone of betrusted-io/xous-core.
- dc34-vault Cargo.toml references xous-core at this exact rev for bao1x-hal, blitstr2, modals,
  ux-api, usb-bao1x, and other crates.
- The pin is detached HEAD — no branch, no fork.

## Tier 1 build reproduction (step by step)

Run all commands from a single base directory. Each step operates from that base directory unless
a subshell `(cd ... && ...)` is used.

1. `(git clone https://github.com/bunnie/dc34-vault && cd dc34-vault && git checkout 7954e6200df67580795b12602e1a7235ed434ca6)`
2. `(git clone https://github.com/bunnie/dc34-api && cd dc34-api && git checkout 617f0f3dff3cea1e9421d766b19664f5bec9a54b)`
3. `(git clone https://github.com/bunnie/dc34-console && cd dc34-console && git checkout bf64e03f019532cca5055fcdbe51977d572e3630)`
4. `(git clone https://github.com/betrusted-io/xous-core && cd xous-core && git checkout 616bf65f6e379165464f50b1e79ec42aff77a683)`
5. `(cd xous-core && cargo xtask install-toolkit)`
6. `(cd xous-core && cargo xtask baosec-lite)`

## Tier 1 revert

To revert to stock DEFCON firmware, reflash the original DEFCON dc34-vault build. Tier 1 changes
are entirely in dc34-vault; xous-core (and therefore xous.uf2) is used verbatim from the DEFCON
pin and is never modified.

PDDB data (gene data, passwords, TOTP) is preserved through any UF2 flash. Bookmarks stored in
`vault.bookmarks` also persist on-device but are not accessible from stock firmware; reinstalling
Tier 1 firmware restores access to them.

Developer mode must never be enabled — it erases Ko (the gene exchange key) and is a one-way door.

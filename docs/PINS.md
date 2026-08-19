# Dependency Pins

- dc34-vault: 7954e6200df67580795b12602e1a7235ed434ca6
- dc34-api: 617f0f3dff3cea1e9421d766b19664f5bec9a54b
- dc34-console: bf64e03f019532cca5055fcdbe51977d572e3630
- xous-core: 616bf65f6e379165464f50b1e79ec42aff77a683 (pinned by this repo's Cargo.lock, NOT HEAD)

## What this fork builds against

The pins below are upstream's, for reproducing the stock DEFCON build. **This firmware does
not build against them.** It needs `heckseven/xous-core` at branch `s-cam`
(`06c90f058` as of 2026-08-18), which carries commits absent from betrusted-io's tree:

| commit | why it is needed |
|---|---|
| `06c90f058` | names the USB device `S-CAM`, which is how you tell running firmware from the bootloader (`Baochip-1x`) |
| `2e31860a8` | retries a refused serial write instead of dropping the rest - pass verdict, build 77 |
| `dab635f0b` | applies back-pressure to bulk IN endpoints - the fix build 76 got half right |

`s-cam` is that fork's default branch, so `git clone` gets it without a checkout step. It is
named here anyway: the dependency is otherwise invisible, and the commits are **not** on the
fork's `main`, which is a stale artifact of the original fork and is kept only so nothing that
once referenced it breaks.

All four repos in this line use `s-cam` as their default branch. The branch was renamed from
`spike/acquire-frame` on 2026-08-18 - a spike name is a poor thing for production firmware to
depend on. GitHub redirects the old name, so existing clones keep working.

## Fork naming

The `dc34-*` names below are upstream's, and the repos and directory names in this document are
upstream's too — the clone commands reproduce the **stock DEFCON build**, so they are left exactly
as they were. This line's forks are named differently:

| upstream repo | this fork | crate / binary name |
|---|---|---|
| `bunnie/dc34-vault` | `s-cam` | `dc34-vault` (unchanged) |
| `bunnie/dc34-api` | `s-cam-api` | `dc34-api` (unchanged) |
| `bunnie/dc34-console` | `s-cam-console` | `dc34-console` (unchanged) |
| `betrusted-io/xous-core` | `xous-core` | — |

Only the directory names changed; the crate names inside the manifests did not. To build *this*
fork rather than stock, check the forks out side by side under those names and run
`./tools/build.sh` from the `s-cam` checkout — see the repo README.

## Reproduction

Run each block from a fresh working directory (each `cd` is relative to that base). These clone
upstream, not this fork:

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
- This repo's Cargo.toml references xous-core at this exact rev for bao1x-hal, blitstr2, modals,
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
are entirely in the vault app (this repo); xous-core (and therefore xous.uf2) is used verbatim
from the DEFCON pin and is never modified.

PDDB data (gene data, passwords, TOTP) is preserved through any UF2 flash. Bookmarks stored in
`vault.bookmarks` also persist on-device but are not accessible from stock firmware; reinstalling
Tier 1 firmware restores access to them.

Developer mode must never be enabled — it erases Ko (the gene exchange key) and is a one-way door.

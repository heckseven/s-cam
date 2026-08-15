# Phase 1 spike results

Both spikes live on xous-core branch `spike/acquire-frame` (which builds on
`spike/departure-mono`). Neither is merged — they exist to answer cost and feasibility.

## 1.5 — Departure Mono placement

**Cost to the app budget: 0 bytes.** Verified by build, not assumed.

```
before adding GlyphStyle::DepartureMono : 0x129b54 = 298 pages
after                                    : 0x129b54 = 298 pages
```

dc34-vault imports only the `GlyphStyle` enum (`ux.rs:7`) and ships text as `TextView`s over
IPC via `gfx.draw_textview()`. It performs no glyph rendering: a search for direct
`blitstr2` blit or glyph calls returns nothing. All rendering is in the flash-resident
graphics server, so font data lands in a different budget entirely.

### What still has to be built

The pager's font pipeline does **not** produce what blitstr2 consumes.

| | produces / consumes |
|---|---|
| `pager-from-heck/scripts/gen_font.py` | a `.bf` binary pack — header plus fixed-cell glyphs |
| blitstr2 | a generated Rust module: `MAX_HEIGHT: u8`, `CODEPOINTS: [u32; N]`, `GLYPHS: [u32; M]`, `WIDTHS: [u8; N]` |
| blitstr2's own codegen (`libs/blitstr2/codegen/main.go`) | a **PNG sprite sheet**, via Go — not an OTF |

So the remaining work is an OTF → blitstr2-Rust generator. Tractable: the geometry is known
(7x14 cell, baseline 11, 95 glyphs, ASCII 32–126) and Departure Mono is monospace, so every
entry in `WIDTHS` is 7. That is implementation work, not spike work.

The style mapping currently mirrors `Tall` as a placeholder so the tree builds; it renders the
existing font until the real glyph module lands.

## 1.6 — Camera frame capture

**Cost to the app budget: 0 bytes.** The opcode is a variant; the work is server-side.

The design is cheaper than originally planned because the work is already being done. The
camera path thresholds each frame to black and white and blits it to the panel
(`bao-video/src/main.rs:89,127` `blit_to_display`; frame buffer at `:434`; `cam.rx_buf()` at
`:649`). `Oled128x128::buffer()` (`bao1x-hal/src/sh1107.rs:421`) returns that framebuffer as
`[u32; WIDTH * HEIGHT / 32]` = **`[u32; 512]` = 2,048 bytes**.

That is bit-identical to the badge's compiled-in bitmap format, so a captured photo can be
stored and re-rendered through the **existing** bitmap path with no new image processing, and
a capture can be set as the standby image directly.

The earlier plan to downsample to 64x60 is dropped: it came from PLAN.md's retired ASCII-art
feature, has the wrong aspect ratio for a 256x240 (~1.07:1) source, and would look worse than
what is already on the screen.

## The durable finding: two unpinned cross-process enums

Both `GlyphStyle` and `GfxOpcode` are serialised between dc34-vault and the graphics server,
and neither had a discriminant guard — unlike `VaultOp`, which `src/vault_api.rs` pins
deliberately.

**`GlyphStyle`** is now pinned. The guard was verified to work by changing a value:

```
error[E0080]: evaluation panicked: assertion failed: GlyphStyle::Tall as isize == 7
```

**`GfxOpcode` is the more dangerous of the two and is still unpinned.** It uses *implicit*
discriminants and ten of its variants are `#[cfg(feature = ...)]`-gated:

```rust
#[cfg(feature = "board-baosec")]
AcquireQr,
KeyPress,
```

So its wire numbering is a **function of the enabled feature set**, not just of variant order.
Inserting a variant, or building the app and the graphics server with different features,
silently renumbers every opcode after the change. This works today only because `cargo xtask
baosec-lite` builds both from one source tree with one feature set.

`AcquireFrame` was therefore **appended** before the `InvalidCall` gutter, matching how
`DryRun` was added, and gated `board-baosec` to match `AcquireQr`.

Pinning `GfxOpcode` is worth doing but is not a one-liner: the assertions must be written per
feature combination, or the enum given explicit discriminants. Recommended before anyone adds
another opcode.

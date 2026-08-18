# Animated bling — shelved, with the numbers

Sized up 2026-08-17 and deliberately not built. Recorded so the feasibility work does not
have to happen twice.

## It is feasible

Most of the machinery exists: a redraw pump on a 250ms tick (`totp::pumper`), a per-screen
`should_animate()` gate, the `bitmap_diffusion` draw path, and PDDB storage that already
holds 2048-byte frames — a photo *is* a frame.

## The constraints that decide the design

* **~4fps as things stand.** The pump is 250ms. Faster is possible, but the panel is SPI and
  `sh1107: timeout in draw` has already been seen under load — continuous full-frame blits
  are exactly that workload.
* **1bpp, 128x128.** A GIF must be thresholded or dithered. Motion survives that; detail does
  not.
* **2KB per frame.** A 12-frame loop is 24KB in the PDDB, which is fine — but `PHOTO_CAP` is
  27, so frames must not land in the photo store or they eat that budget. Animation needs its
  own dictionary.
* **29 pages of headroom** against the 307-page loader limit. Enough for the code, not enough
  to hold frames resident, so they stream from the PDDB per tick — more work on the
  demand-paged path every frame.
* **Power and stability.** The standby screen repaints only on change (`standby_drawn`).
  Animating it means giving that up on the screen the badge sits on for hours.

## What it would need

1. A real bling store. There is not one today: "bling" is the two built-in images plus the
   photo list, and `set_photo_as_bling` just picks the standby image.
2. A frame-cycling idle draw, bypassing the repaint-on-change optimisation.
3. GIF splitting in the uploader — Pillow reads frames, so this is the easy part.

## How to approach it

Prototype "animated standby" with a hard cap — 8 frames at 4fps — before designing anything.
One build answers the only question that matters: whether the panel and a swap-paged app can
sustain continuous full-frame redraws. The code is not the risk.

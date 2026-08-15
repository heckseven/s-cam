# S-CAM Design Decisions

Resolved with the user on 2026-08-15. These supersede `agent-todo.md` where they conflict.
Rationale is recorded because several of these reverse an earlier assumption.

## Scope

| # | Decision | Notes |
|---|---|---|
| 1 | **DEFCON fidelity is NOT required** | A single combined UI is preferred over carrying two UI paths. Cutting DEFCON scope to buy size budget is acceptable. |
| 2 | **Gene QR exchange is CUT** | Was protected, then cut. Frees 4 `VaultMode` variants, `genemenu.rs`, the k0/nonce/`DC34_HEADER` paths, and probably the whole `aes_gcm_siv` and `base45` dependencies. |
| 3 | **LED mutation is the sole protected feature** | `mutate()` / `render_gene()` / lightgenes in dc34-console. Never touches k0, so unaffected by the key wipe. |
| 4 | **Tour, TokenTour, FactoryTest, StandAloneTest are cuttable** | Subject to the `FactoryNew` state-machine caveat below. |
| 5 | **FIDO2 is kept** | Not cut. Revisit only if measured demand exceeds supply. |
| 6 | **"DEFCON mode" menu item is dropped** | It existed for gene exchange. The DEFCON logo survives as a standby-image option. |

## Menu tree

Three credential types exist and were previously indistinguishable in the UI. Each menu
name now maps to exactly one store:

| Menu item | Store | Previously |
|---|---|---|
| `PASSWORDS` | `vault.passwords` | absent from the specified tree |
| `2FA DIGITS` | `vault.totp` | called "tokens" (ambiguous) |
| `PASSKEYS` | `fido.u2fapps` + OpenSK store | **no screen at all** — stored but invisible |
| `BOOKMARKS` | `vault.bookmarks` | unchanged |
| `PHOTOS` | new store | new |
| `SETTINGS` | bling / blinky / screen off | unchanged |
| `ABOUT` | QR to the repo README | unchanged |
| `EXIT` | closes the menu → standby | was undefined ("exit" to what?) |

- **`PASSKEYS` is a new screen.** FIDO2 credentials are stored today but have no UI; that
  invisible third type is the main source of the current UX confusion.
- **`2FA DIGITS`** was chosen over "TOTP" / "CODES" / "2FA CODES" for non-technical clarity.
- **`EXIT`** maps to the existing `MenuDone` op, not `PowerOff` — "screen off" already lives
  under settings and two shutdown-sounding paths would be confusing.

## Photos

- **Capture the 128x128 black-and-white frame exactly as displayed.** `bao-video` already
  thresholds the camera frame and blits it to the OLED; capturing means grabbing that
  buffer. 2,048 bytes — bit-identical to the format the badge's existing bitmaps use.
- **Cap: 27 photos** (~55 KB, roughly 1.5% of the 4 MiB PDDB). Storage is not the
  constraint; browsing UI code size is.
- Do **not** downsample to 64x60 as earlier planned — that figure came from PLAN.md's
  retired ASCII-art feature, has the wrong aspect ratio, and looks worse than the preview.

## Standby images

- **Separate store from photos.** Photos and standby images share a format but not a purpose.
- Three input paths:
  1. **Camera capture** → "set as standby". Nearly free; the format already matches.
  2. **`dc34-image` USB upload** → works today, writes `"dc34"/"image"`. Needs documenting,
     possibly extending beyond its single slot.
  3. **Single QR at 64x64 + RLE compression**, upscaled 2x for display. Full 128x128 would
     need 5-6 chained QRs: the camera is 256x240 and QR decoding needs >=3 px/module, which
     caps a reliable scan at ~v17 / ~400 bytes.
- The DEFCON logo and the S-CAM image remain built-in options.

## Bookmarks

- **A bookmark is marked as default explicitly** from the bookmarks list, stored in settings.
  Previously undefined, despite the idle-screen left button depending on it.

## Implementation

| # | Decision | Rationale |
|---|---|---|
| 1 | `BADGE_NAME = "S-CAM"`, single constant | Name may change; repo will be renamed to something S-CAM-like. |
| 2 | **Plain Rust constants for new UI strings — not `t!()`** | Avoids the locales trap: `dc34-vault/locales/i18n.json` is never read, and patching it would force every new string into the shared xous-core image. Translation is not needed. |
| 3 | Departure Mono **11px only** | 22px comes free later via integer 2x scaling of the same pack. |
| 4 | Font lives in the flash-resident gfx server | Text renders via `gfx.draw_textview()`, so the font costs the app's page budget ~0. |

## Constraints that shape all of the above

- **dc34-vault must stay <= 307 pages**, currently 298 with `incremental = false`
  (9 pages headroom). Over the limit, the badge hangs at a textless progress bar with no
  error and needs reflashing.
- **`k0` is already wiped** and is one-way — it happened at the first custom flash. It was
  the DEFCON population-wide shared key, and only gene exchange used it. Now moot except
  that `config.rs:136-142` derives `AttachState::FactoryNew` from it, so **removing k0
  handling is a state-machine change, not a deletion**.
- **Cutting `FactoryTest` requires adding an explicit `FactoryNew` arm** to
  `config.rs:172-195` in the same commit — `FactoryNew` is derived every boot, not a
  factory-only state.

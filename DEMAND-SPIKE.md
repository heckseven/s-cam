# Demand measurement (task 1.7) — multiplier declared before measuring

## Residual multiplier: 3.0x

**Declared before the first build.** This file is committed with that value, and the number
is not revised afterwards regardless of what the skeleton measures. A multiplier chosen after
seeing the answer is a fudge factor fitted to the answer, and would make the demand figure
worthless for deciding anything.

### Why 3.0

A logic-free skeleton already contains the parts that are hard to shrink: enum variants,
exhaustive `match` arms, menu item structs, screen entry points, and the string literals for
labels. What it omits is the bodies — PDDB reads and writes, list paging, error handling,
formatting, and the state transitions between screens.

For UI-heavy embedded code where screens mostly display and navigate, the bodies typically run
2–4x the scaffolding. 3.0x is the middle of that band. It is a judgement, not a measurement,
and it is stated as such.

**Both figures get reported separately** — raw skeleton and multiplied — so the reader can
apply their own multiplier and see how sensitive the conclusion is to mine.

## Method

Each feature is added on top of the previous one, and the ELF is measured after each step, so
every line is an independent price. All builds use the reproducible configuration
(`incremental = false`, dedicated `CARGO_TARGET_DIR`).

Measured against the **legacy-cut** baseline of 276 pages, not the 298-page current build —
that is the tree these features will actually land in.

## Results

Measured against the legacy-cut baseline, `top_vaddr` 1,126,724 (276 pages).

| feature | bytes | pages |
|---|---:|---:|
| 9 new `VaultMode` variants + `should_animate` + power arms + 9 distinct screens (heading, list rows, button-label bar) | ~4,312 | 1.05 |
| 8-item menu tree | ~1,048 | 0.26 |
| photo store + standby-image store + RLE decoder + 2x upscaler | 1,976 | 0.48 |
| S-CAM bitmap (`[u32; 512]`, exact) | 2,048 | 0.50 |
| **skeleton total** | **9,384** | **2.29** |
| **x3.0 declared multiplier** | **28,152** | **6.87** |

The first two rows are approximate individually — the skeleton was rebuilt partway through
(see below) — but their sum is measured, and the last two are exact.

### Verdict

| | pages |
|---|---:|
| available after the legacy cut | 31 |
| less the 2-page flash margin | **29 usable** |
| predicted demand at 3.0x | **6.9** |
| remaining | **~22** |

**Demand fits roughly four times over.** Even at a 5x multiplier the figure is 11.5 pages,
still less than half the available headroom. That retires both damaging levers:

- **FIDO2 stays.** The badge remains a USB security key.
- **Unwind metadata stays.** Panics stay debuggable, which matters on a device whose only
  diagnostic channel is an unattached UART.
- **Gene exchange did not need cutting either** — the legacy screens alone paid for
  everything. That is now a product choice, not a budget one.

### A correction made mid-measurement

The first skeleton gave all nine screens a **single shared match arm** with a small inner
`&str` lookup, and measured **+112 bytes**. That figure was discarded rather than reported:
nine real screens do not share one body, and multiplying an unrepresentative skeleton would
have produced a falsely low demand number and could have driven a wrong Gate A decision.

The skeleton was rebuilt with a distinct arm per screen — each drawing a heading, list rows
where the screen is a list, and a three-slot button-label bar — which is what real screen
scaffolding looks like. All figures above come from that version.

### What the skeleton still does not contain

Real bodies: PDDB error paths, list paging, the camera capture path itself, credential
enumeration for `PASSKEYS`, and the settings persistence logic. Those are precisely what the
3.0x multiplier is for. It remains a judgement, and the raw figure is given so it can be
re-judged.

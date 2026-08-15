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

Filled in by the spike. Empty until measured.

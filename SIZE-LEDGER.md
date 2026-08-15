# Size ledger — measured, not estimated

Baseline: `incremental = false`, reproducible. Two clean builds of identical source produce a
byte-identical ELF.

```
top_vaddr  0x129b54 = 1,219,412 bytes = 298 pages
limit                              307 pages
headroom                             9 pages
usable (after the 2-page flash margin)  7 pages = 28,672 bytes
```

## Where the bytes are (from `llvm-size -A`)

| section | bytes | pages | share |
|---|---:|---:|---:|
| `.text` | 736,072 | 179.7 | 64.3% |
| `.rodata` | 179,912 | 43.9 | 15.7% |
| `.gcc_except_table` | 109,160 | 26.6 | 9.5% |
| `.eh_frame` | 95,768 | 23.4 | 8.4% |
| `.eh_frame_hdr` | 23,356 | 5.7 | 2.0% |
| `.data` + `.bss` | 1,128 | 0.3 | 0.1% |
| **total** | **1,145,613** | | |

Section total is below `top_vaddr` because of inter-section alignment padding.

**Unwind metadata = 228,284 bytes = 55.7 pages**, spread across three sections and carrying no
symbols at all, which is why it is invisible to `nm`. That is over six times the entire usable
headroom, in data that exists only to make panics unwind — on a device whose only diagnostic
channel is a UART nobody has attached.

## Bitmaps — resolved

**33 anonymous read-only symbols of exactly 2,048 bytes.** Each of the 33 bitmaps is emitted
exactly once; LLVM merged the per-use-site promotions, so the naive figure was correct.

| group | count | bytes | pages |
|---|---:|---:|---:|
| `tour_*` | 18 | 36,864 | 9.0 |
| `factory_*` | 9 | 18,432 | 4.5 |
| kept (`dc_logo`, `lowbatt`, `badge_flip`, `baochip_about`, `bunnie`, `cheeso`) | 6 | 12,288 | 3.0 |
| **total** | **33** | **67,584** | **16.5** |

Bitmaps are **37.6% of all `.rodata`**. Cutting tour and factory removes **55,296 bytes
(13.5 pages)** of pure data — nearly twice the usable headroom, before any code is touched.

This needed measuring rather than computing: `pub const BITMAP: [u32; 512]` emits nothing until
referenced, and `&bitmaps::X::BITMAP` triggers rvalue static promotion at each use site, so
duplication was plausible. It did not happen.

## Supply confirmed so far

| lever | bytes | pages | status |
|---|---:|---:|---|
| tour + factory bitmaps | 55,296 | 13.5 | **measured** |
| tour/factory/gene code paths | — | — | pending a gated build |
| unwind metadata | 228,284 | 55.7 | measured; contingency only, latent failure |
| profile tuning | 0 | 0 | already `z` / `fat` / 1 / non-incremental |

The bitmap line alone is close to twice the usable headroom, which is why cutting tour and
factory is the leading option and FIDO2 stays untouched.

## MEASURED: legacy cut (task 1.4)

Removed `VaultMode::{Tour, TokenTour, FactoryTest, StandAloneTest, DefconHelp, TokenHelp,
About}`, their `ux.rs` arms, their `config.rs` boot routing, and the 27 `tour_*`/`factory_*`
bitmaps. Built and measured on branch `measure/legacy-cut`. **Gene exchange was left in.**

```
baseline    0x129b54 = 298 pages,  9 pages headroom
legacy cut  0x113144 = 276 pages, 31 pages headroom
reclaimed   92,686 bytes = 22.6 pages
```

| section | base | cut | delta |
|---|---:|---:|---:|
| `.text` | 736,072 | 714,594 | −21,478 |
| `.rodata` | 179,912 | 112,052 | **−67,860** |
| `.gcc_except_table` | 109,160 | 107,564 | −1,596 |
| `.eh_frame` | 95,768 | 94,360 | −1,408 |
| `.eh_frame_hdr` | 23,356 | 23,012 | −344 |
| **total** | 1,145,613 | 1,052,927 | **−92,686** |

`.rodata` gives up more than the 55,296 bytes of bitmaps alone — the balance is string
literals and tables belonging to the removed screens.

**Usable headroom goes from 7 pages to 29.** That is the number that decides Gate A, and it
strongly suggests neither of the two damaging levers is needed: FIDO2 can stay, and unwind
metadata can stay.

### LED mutation survives — verified, not assumed

The one way this cut could silently break the sole protected feature was if local light-gene
storage depended on the cipher. It does not:

- `cipher()` has exactly two call sites, `main.rs:708` and `main.rs:762`, both gene
  *exchange* encrypt/decrypt.
- `save_light_gene()` / `get_light_gene()` (`dc34-api/src/lib.rs:424,445`), `mutate()`
  (`:537`) and `render_gene()` (`config.rs:374`) never touch `cipher()` or `k0`. The local
  gene is stored in plaintext.

### Caveat

This measurement routes the removed boot states to `Idle` so it compiles. That is **not** a
design — `config.rs:136-142` still derives `AttachState::FactoryNew` from `k0`, and cutting
`FactoryTest` leaves the production arm without a target. The real routing is a state-machine
decision, not an asset removal.

## Method

```
llvm-size -A <elf>                     # section ground truth
llvm-nm --size-sort --radix=d -S <elf> # symbol attribution
```

Bitmaps are identified by signature — anonymous, read-only, exactly 2,048 bytes — because
`pub const` items carry no name in the symbol table.

## MEASURED: flash budget and PDDB (task 1.9)

### The ja/emoji trim yields zero — drop it

It was proposed on the assumption that blitstr2's Japanese and emoji glyph sets bloat the
flash-resident image. They do not, because they are never linked:

```
bao-video .rodata total          284,328 bytes
ja CODEPOINTS[11081] + GLYPHS[88648]  398,916 bytes
```

The entire read-only section of the graphics server is smaller than the Japanese tables alone,
so they cannot be present. They are `pub const`, and Rust discards unreferenced const data —
the same reason the app's bitmaps had to be measured by size signature rather than by name.

**Gate A Q5 resolves to zero.** Dropping it removes one edit from the resident-image fork.

### Flash / resident-image budget

```
kernel image        3,187,216 bytes
free remaining        196,848 bytes  = 48 pages
```

That is the budget the LED pattern engine, Departure Mono and `AcquireFrame` draw on. All
three are small against 48 pages, and it is a separate pool from the app's 307-page limit.

### bao-video sections, for reference

| section | bytes |
|---|---:|
| `.rodata` | 284,328 |
| `.text` | 204,352 |
| `.eh_frame` | 27,744 |
| `.gcc_except_table` | 15,560 |
| `.eh_frame_hdr` | 5,636 |

Two anonymous blocks of 65,536 bytes each are `rqrr`'s Galois-field multiply tables
(`GF16_mod`, `GF256_mod`) — QR decoding, not fonts.

### PDDB: nine keys in dict `dc34`

| key | vault | console | notes |
|---|:--:|:--:|---|
| `k0` | 1 | 0 | wiped and one-way. Sole remaining role is deriving `AttachState` at `config.rs:136-142` |
| `gene` | via api | via api | reached through `get_light_gene`/`save_light_gene`, not the constant. **Protected feature** |
| `badge` | 1 | 2 | badge type |
| `image` | 2 | 3 | existing standby image, `[u32; 512]` |
| `tour` | 3 | 2 | in the legacy cut's blast radius (`set_skip_tour`) |
| `tokentour` | 2 | 0 | in the legacy cut's blast radius |
| `bio.code` | 0 | 4 | **console only** — BIO program, read up to 0xf00 bytes |
| `bio.pins` | 0 | 4 | console only |
| `bio.clk` | 0 | 5 | console only |

The three `bio.*` keys are written and read solely by dc34-console. Rebuilding the console
does not by itself invalidate them — they are PDDB data, not compiled-in — but any change to
the BIO program's expected layout would, and that is worth checking before the LED work lands.

Cutting tour and factory should also drop `tour` and `tokentour` writes; the keys become inert
rather than harmful.

### Photo and image storage headroom

```
27 photos            55,296 bytes   1.32% of the 4 MiB PDDB
+ 27 standby images 110,592 bytes   2.64%
page-granular worst 221,184 bytes   5.27%
```

The 27 cap is conservative even at page granularity. Storage was never the constraint —
browsing-UI code size is.

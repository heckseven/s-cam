# How to verify changes to this firmware

## There is no automated test path. This is measured, not assumed.

| approach | status |
|---|---|
| `cargo test` | **fails** — `utralib` build script panics under `hosted-baosec`; `keystore-api` fails to compile |
| `cargo build --features hosted-baosec` | **fails** — `error[E0080]: evaluation panicked` in the `utralib` build script |
| compile-time assertions | **works** — used to pin cross-process opcode discriminants (`src/vault_api.rs`) |
| host-side scripts against the ELF | **works** — `check-app-size.py` |
| host-side scripts against the source | **works** — `tools/check-menu-wiring.py` |
| on-hardware human checklist | **works** — the only way to verify behaviour |

Why hosted fails: `Cargo.toml:41` pins `bao1x-hal` with `features = ["board-baosec",
"oem-baosec-lite"]` **unconditionally**, while the crate's `hosted-baosec` feature selects
`ux-api/hosted-baosec` and never `bao1x-hal/hosted-baosec`. A hosted build therefore requests
two conflicting `ux-api` board sets plus a RISC-V UTRA on x86. Making it work means untangling
that feature graph — a project in its own right, not a prerequisite for this one.

**Consequence:** anything whose success criterion says "verify in hosted mode" is really a
human checklist. They are written below so nobody discovers this mid-implementation.

## Source-level checks worth having

`cargo test` cannot run here, so the checks that exist are host-side scripts run against the
source or the ELF. Both are cheap and both have been verified to *fail* on the bug they
describe — a check that has only ever passed is not evidence.

- `python3 check-app-size.py <elf>` — page budget. Over the limit the badge hangs partway
  through its boot progress bar with no error and must be reflashed.
- `python3 tools/check-menu-wiring.py` — every idle-menu entry reaches a handler that can
  actually receive it. Catches four ways an entry goes dead: an opcode `main.rs` does not
  handle, two entries sharing one opcode, a `msg_blocking_scalar_unpack!` handler that can
  never match the widget's non-blocking sends, and a handler that fails to clear
  `menu_active` (which leaves every later key routed into the closed menu).
- `tools/build.sh` — builds the apps *before* bundling. `cargo xtask` bundles whatever ELFs
  it is pointed at and does not build them, so calling it directly packages a stale binary
  and still prints a clean build.

## Checklists

Run each against a badge flashed with the build under test. Record pass/fail per line — a
partial pass is a fail, because the failure mode we care about is silent.

### Departure Mono font

- [ ] Text renders in Departure Mono, not the previous font (compare letterforms against
      `~/Downloads/DepartureMono-1.500/`)
- [ ] Glyphs are white on black
- [ ] Headings are ALL CAPS
- [ ] Text is legible at arm's length on the 128x128 panel
- [ ] No clipped descenders on `g j p q y`
- [ ] A full-width string of `W` does not overflow the panel

### Camera frame capture (`AcquireFrame`)

- [ ] Camera preview appears when opened
- [ ] Pressing capture returns to the previous screen without hanging
- [ ] The captured image matches what was on screen at the moment of capture
- [ ] **Scanning a QR code still works after a capture** — this is the regression that
      matters; `AcquireFrame` shares the camera with `acquire_qr()`
- [ ] Capturing twice in a row works (no leaked deferred message on the second attempt)
- [ ] Capturing, then leaving the camera, then re-entering, works

### Idle-screen buttons

Run for **every** idle state, not just the one on your bench — `Idle`, `IdleDevMode`
(permanent on a developer-mode badge), and whatever the `Unattached` cell resolves to.

- [ ] Left shows the default bookmark as a QR
- [ ] Left again returns to standby
- [ ] Middle opens the camera
- [ ] Right cycles LED patterns
- [ ] In camera: right captures, middle exits, left shows the bookmark QR
- [ ] Every path returns to standby with the animation running again
- [ ] No path leaves a QR on screen after returning to standby

### Settings and standby images

- [ ] Each of `bling` / `blinky` / `screen off` applies immediately, without a reboot
- [ ] Each survives a power cycle
- [ ] A camera capture can be set as the standby image
- [ ] The standby image survives a power cycle
- [ ] The DEFCON logo and the S-CAM image are both selectable

### Photos

- [ ] A photo persists across a power cycle
- [ ] **Photograph a printed high-contrast letter; that letter is identifiable in a photograph
      of the badge screen** — this is the stated pass condition, deliberately not
      "looks recognisable"
- [ ] The 27-photo cap is enforced at capture, and refusing is graceful
- [ ] Deleting a photo frees its slot

### Always, before any flash

- [ ] `./check-app-size.py <elf>` reports **at least 2 pages of margin**
- [ ] The staged queue's last entry is the known-good triple

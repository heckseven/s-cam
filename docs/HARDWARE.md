# Hardware Go/No-Go Assessment

Verified against xous-core@616bf65f, dc34-vault@7954e62.

---

## Hosted Emulator (`baosec-emu` / `hosted-baosec` feature)

### HID Output — NO-GO

**Finding:** Hosted mode does NOT emit real HID output.

- `xous-core/services/usb-bao1x/src/main_hosted.rs` line 146: `Some(Opcode::SendString) => {}`
- The `SendString` opcode handler body is empty — no bytes are forwarded to any HID interface.
- The `acquire_qr` call via `Gfx::acquire_qr()` (`xous-core/libs/ux-api/src/service/gfx.rs` line ~741) still routes through the IPC call, but the USB HID layer that would emit keystrokes on hardware does nothing in hosted mode.
- **Implication for Task 5 (HID URL type-out):** Acceptance testing of actual USB HID output requires a physical badge. The hosted emulator cannot verify that bytes leave the USB port.

### Camera Frames — NOT REAL

**Finding:** Hosted mode does NOT return real camera frames.

- `xous-core/libs/ux-api/src/service/gfx.rs` hosted `acquire_qr()` returns a hardcoded dummy TOTP URI (see that file's `acquire_qr` hosted impl for the literal). The secret field is a valid Base32 string but belongs to a clearly fictional identity — documented here only as structure, not transcribed.
- This is NOT a zeroed buffer — it returns a plausible QR payload that triggers the parsing code path.
- `bao-video/src/main.rs` line ~378: hosted camera is initialized as `unsafe { Gc2145::new() }` from `bao1x_emu` — a software emulator, not a real sensor.
- **Implication for Task 6 (bookmark-to-QR):** The hosted dummy always returns a TOTP URI. Testing round-trip for a bookmark URL requires either mocking `acquire_qr` or a physical badge. The hosted path exercises the code flow but not real scanning.

---

## Physical Badge Requirements

The following acceptance steps are flagged **HUMAN-PENDING** and cannot be cleared without a physical badge:

- **Task 5 — HID URL type-out:** Confirm the exact displayed bytes exit the USB port with no trailing newline. Confirm declining emits nothing. Confirm existing password autotype is unaffected. These require physical USB HID observation (e.g., a USB sniffer or a connected host watching keyboard events).
- **Task 6 — bookmark-to-QR:** Confirm a bookmark at `CAP_BOOKMARK_URL` (defined in CAPACITY.md, a Task 2 deliverable) renders correctly via the camera's optical path and that the QR code round-trips byte-identically. Real camera required.
- **Tasks 8–9 — regression + docs:** Physical badge required to confirm no regression in existing autotype and QR scanning behavior.

---

## Developer Mode — ONE-WAY DOOR

**Developer mode must never be enabled on any badge used for testing.**

- Enabling developer mode erases `k0` (the `DC34_SECRET` key) and cannot be undone.
- This is a permanent hardware state change; there is no recovery path.
- All build and test steps in this swarm use `hosted-baosec` or `board-baosec` (OEM lite) features only.
- No code path added in Tier 1 may trigger developer mode or call any function that writes to the developer-mode PDDB key.

---

## Build Status

- `cargo xtask install-toolkit` (xous-core): **BLOCKED** — requires explicit authorization to execute code from externally-cloned repos. Run this step manually in a trusted shell.
- `cargo xtask baosec-lite` (xous-core): **NOT RUN** — depends on install-toolkit completing first.
- `cargo xtask baosec-emu` (xous-core): **NOT RUN** — same blocker.

**All source-level work (symbol verification, repo pinning) is complete and correct.** The build gate itself requires user authorization to execute external repo code.

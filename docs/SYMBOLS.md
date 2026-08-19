# Symbol Map — DC34 Tier 1

All paths are relative to the sibling repo root (s-cam/, s-cam-api/, xous-core/).
Verified against the upstream pins: dc34-vault@7954e62, dc34-api@617f0f3, xous-core@616bf65f.
dc34-console@bf64e03 has no symbols relevant to Tier 1 tasks and is not listed below.
The **Repo:** labels below use the crate names, which did not change in this fork: `dc34-vault`
is checked out as `s-cam/`, `dc34-api` as `s-cam-api/`, `dc34-console` as `s-cam-console/`.

---

## Symbols

### `VaultMode` (enum + `should_animate` impl)
- **Repo:** dc34-vault
- **File:** `src/main.rs`
- **Declaration:** Line ~56, `pub enum VaultMode { Idle, IdleDevMode, ShowKey { quantum: u32 }, ResponseGene { quantum: u32 }, ConfirmGene, GeneScan, FactoryTest, StandAloneTest, Tour, TokenTour, DefconHelp, About, Totp, Password, TokenHelp }`
- **`should_animate` impl:** Line ~75, `pub fn should_animate(&self) -> bool` — implemented directly on `VaultMode` in the same file.
  - Returns `true`: About, FactoryTest, StandAloneTest, DefconHelp, TokenHelp, Idle, IdleDevMode, Totp, GeneScan, ResponseGene { .. }, ShowKey { .. }, Tour, TokenTour
  - Returns `false`: ConfirmGene, Password

### `ActionOp::AcquireQr`
- **Repo:** dc34-vault
- **File:** `src/actions.rs`
- **Declaration:** Line ~62, `AcquireQr,` — plain variant (no fields) in the `ActionOp` enum, under a `/// QR ops` comment. No cfg guard.

### `GfxOpcode::AcquireQr`
- **Repo:** xous-core
- **File:** `libs/ux-api/src/service/api.rs`
- **Declaration:** Line ~104, `#[cfg(feature = "board-baosec")] AcquireQr,` — variant inside `pub enum GfxOpcode`, gated on `board-baosec` feature.

### `QrAcquisition` struct
- **Repo:** xous-core
- **File:** `libs/ux-api/src/service/api.rs`
- **Declaration:** Lines ~139-142:
  ```rust
  #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
  pub struct QrAcquisition {
      pub content: Option<String>,
      pub meta: Option<String>,
  }
  ```
- Both fields are `pub`. Camera raw frames are NOT exposed — only decoded content as `Option<String>`.

### `qr_override` field
- **Repo:** dc34-vault
- **File:** `src/ux.rs`
- **Declaration:** Line ~528, `pub qr_override: Option<QrCode>,` — field on the main UX struct.
- Initialized to `None` in constructor (~line 595). Read at rendering (~line 921). Written from within ux.rs (~line 1675) and from main.rs (~line 665).

### `Manager::pddb_store`
- **Repo:** dc34-vault
- **File:** `src/storage.rs`
- **Declaration:** Line ~105, `fn pddb_store(` — **private** (no visibility modifier).
- Signature: `fn pddb_store(&self, payload: &[u8], dict: &str, key_name: &str, alloc_hint: Option<usize>, basis: Option<String>, sync: bool, overwrite: bool) -> Result<(), storage::Error>`
- **Action required for Task 4:** Widen to `pub(crate)` to allow access from new PDDB bookmark code.

### `Manager::pddb_get`
- **Repo:** dc34-vault
- **File:** `src/storage.rs`
- **Declaration:** Line ~140, `fn pddb_get(` — **private** (no visibility modifier).
- Signature: `fn pddb_get(&self, dict: &str, key_name: &str) -> Result<Vec<u8>, storage::Error>`
- **Action required for Task 4:** Widen to `pub(crate)` to allow access from new PDDB bookmark code.

### `VAULT_PASSWORD_DICT` and `VAULT_TOTP_DICT`
- **Repo:** dc34-vault
- **File:** `src/vault_api.rs`
- **Declaration:**
  - Line 14: `pub const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";`
  - Line 15: `pub const VAULT_TOTP_DICT: &'static str = "vault.totp";`
- Re-exported via `pub use vault_api::*;` in main.rs. Also imported directly into storage.rs.

### `DC34_DICT`, `DC34_GENE`, `DC34_SECRET`
- **Repo:** dc34-api
- **File:** `src/lib.rs`
- **Declaration:**
  - Line 7: `pub const DC34_DICT: &str = "dc34";`
  - Line 8: `pub const DC34_SECRET: &str = "k0";`
  - Line 12: `pub const DC34_GENE: &str = "gene";`
- **CONSTRAINT:** No new code path may read or write to `DC34_DICT` or use `DC34_GENE` / `DC34_SECRET`. These are out of scope for all Tier 1 tasks.

### `save_light_gene` function
- **Repo:** dc34-api
- **File:** `src/lib.rs`
- **Declaration:** Line ~445, `pub fn save_light_gene(gene: Diploid)` — writes serialized Diploid to PDDB under `DC34_DICT` / `DC34_GENE`.
- **CONSTRAINT:** Must never be called from any new code path (Tier 1 constraint).

### `GlobalConfig::replace_gene`
- **Repo:** dc34-vault
- **File:** `src/config.rs`
- **Declaration:** Line ~365, `pub fn replace_gene(&mut self, egg: Haploid, sperm: Haploid)` — on `pub(crate) struct GlobalConfig`.
- Body: saves current gene to `prior_gene`, then sets `gene_cache`.
- **CONSTRAINT:** Must never be called from any new code path (Tier 1 constraint).

### `char_to_hid_code_us101`
- **Repo:** xous-core
- **File:** `services/usb-bao1x/src/mappings.rs`
- **Declaration:** Line 6, `pub fn char_to_hid_code_us101(key: char) -> Vec<UsbKeyCode>`
- **cfg guard:** `#[cfg(any(feature = "precursor", feature = "renode", feature = "bao1x"))]`
- **Visibility from dc34-vault:** NOT reachable under `board-baosec` — the `bao1x` feature that gates this function is not activated by dc34-vault's `board-baosec` dependency chain. See Q2 below for full analysis.

### `GfxOpcode` enum
- **Repo:** xous-core
- **File:** `libs/ux-api/src/service/api.rs`
- **Declaration:** Lines ~30-129, `pub enum GfxOpcode { ... }`. Full enum with board-baosec-gated variants.
- bao-video (SERVER_NAME_GFX) handles all GfxOpcode variants; it is the gfx server.

### `IMAGE_WIDTH`, `IMAGE_HEIGHT` (camera constants)
- **Repo:** xous-core
- **File:** `services/bao-video/src/main.rs`
- **Declaration:**
  - Line ~71: `pub const IMAGE_WIDTH: usize = 256;`
  - Line ~72: `pub const IMAGE_HEIGHT: usize = 240;`
- These are the physical camera frame dimensions used in the QR acquisition loop.

### `CHAR_WIDTH`, `CHAR_HEIGHT` (bao-video font constants)
- **Repo:** xous-core
- **File:** `services/bao-video/src/gfx.rs`
- **Declaration:**
  - Line ~13: `pub const CHAR_HEIGHT: isize = 12;`
  - Line ~14: `pub const CHAR_WIDTH: isize = 6;`
- 6×12 bitmap font loaded from `loader/src/font6x12_1bpp.raw`.

---

## Design Questions

### Q1: Which UX module compiles under `board-baosec`?

**Answer: `src/ux.rs`** — always active.

`s-cam/src/main.rs` declares `mod ux;` with no `#[cfg]` guard, so it compiles unconditionally. Rust resolves `mod ux;` to `src/ux.rs` because that file exists. The directory `src/ux/` (containing `src/ux/framework.rs` and `src/ux/icontray.rs`) is a separate subtree that is only included if `src/ux.rs` explicitly declares `mod framework;` or `mod icontray;` — it does not. `src/ux/framework.rs` is NOT compiled under any feature configuration.

There is no cfg condition distinguishing the two in Cargo.toml or in the `mod ux;` declaration. The Precursor/framework path is structurally dead code that is not wired into the build.

### Q2: Is `char_to_hid_code_us101` reachable from dc34-vault without editing usb-bao1x?

**Answer: NO — Task 5 (hid-url-typeout) must use a local copy defined in `s-cam/src/sanitize.rs`.**

Detailed analysis:
- `char_to_hid_code_us101` in `xous-core/services/usb-bao1x/src/mappings.rs` is declared `pub` but is gated behind `#[cfg(any(feature = "precursor", feature = "renode", feature = "bao1x"))]`.
- dc34-vault's `board-baosec` feature activates `usb-bao1x/board-baosec`.
- The `usb-bao1x/board-baosec` feature does NOT include `bao1x` as a sub-feature (it enables `utralib/bao1x` and `bao1x-hal`, but the `bao1x` feature flag itself — which gates `char_to_hid_code_us101` — is a separate standalone feature of usb-bao1x).
- Therefore, under the standard `board-baosec` build path, `char_to_hid_code_us101` is compiled out and not accessible from dc34-vault.
- The constraints also prohibit editing usb-bao1x or adding `[patch]` entries.
- **Conclusion:** The character-to-HID mapping must live in `s-cam/src/sanitize.rs` (a Task 3 deliverable alongside `SanitizedUrl`). Task 5 references it from there. This is consistent with the shared context document's pre-decision.

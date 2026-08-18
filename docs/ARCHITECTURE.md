# Architecture — DC34 Vault Tier 1 Extension

This document covers the four architectural decisions introduced by Tier 1. It is intended for a
reader who has no prior context on this codebase but can read Rust.

> Tier 1 makes zero modifications to xous-core. The DEFCON xous.uf2 is used verbatim at pin
> 616bf65f. All new functionality lives in dc34-vault. To revert Tier 1 changes entirely,
> reflash the original DEFCON dc34-vault build.

---

## 1. SanitizedUrl Invariants and URL Caps

### What SanitizedUrl guarantees

`SanitizedUrl` is a newtype over `String` defined in `dc34-vault/src/sanitize.rs`. Its constructor
is the only place in the codebase where a raw scanned string is converted into a value that URL
code paths will consume. The constructor is fallible and performs exactly three checks, in order:

1. **Control characters** — any byte in the range `\x00`–`\x1F` or `\x7F`, including `\n`, `\r`,
   and `\t`, causes the constructor to return `Err`. The string is never modified; it is either
   accepted whole or rejected entirely.

2. **Unmapped HID characters** — every character in the string is looked up in the local HID
   character map (defined alongside `SanitizedUrl` in `sanitize.rs`, mirroring
   `char_to_hid_code_us101` from `xous-core/services/usb-bao1x/src/mappings.rs`). Any character
   that has no mapping returns `Err`. The string is never modified.

3. **Length cap** — the string length in bytes must not exceed the applicable cap. Which cap is
   applied depends on call site (see below). Exceeding the cap returns `Err`.

If all three checks pass, the constructor returns `Ok(SanitizedUrl(...))`. There is no truncation
path anywhere in the constructor. A string is either accepted unchanged or rejected.

### Two caps: CAP_URL_DISPLAY vs CAP_BOOKMARK_URL

Both constants are defined in `dc34-vault/src/capacity.rs` (the source of truth for display and QR
capacity measurements) and re-exported from `dc34-vault/src/sanitize.rs` so callers have a single
import path.

| Constant | Applies to | Binding constraint |
|---|---|---|
| `CAP_URL_DISPLAY` | ShowUrl screen display; HID type-out | The badge's 128×128 display (21 chars/row × ~10 rows) |
| `CAP_BOOKMARK_URL` | Bookmark storage; bookmark-to-QR re-render | QR version renderable on a 128×128 OLED screen |
| `CAP_QR_DISPLAY` | Internal alias; equals `CAP_BOOKMARK_URL` | Same constraint — exported for clarity at call sites |

`CAP_BOOKMARK_URL` equals `CAP_QR_DISPLAY` (the maximum URL length that produces a QR code whose
modules remain large enough for a phone to scan from a 128-pixel display). The encoder itself can
handle multi-kilobyte payloads; the constraint is the display, not the encoder.

`CAP_URL_DISPLAY` may be smaller than `CAP_BOOKMARK_URL` if the rendered text wraps beyond the
usable area of the ShowUrl screen before reaching the QR limit. In that case:

- A URL accepted by the display check (`CAP_URL_DISPLAY`) can always be bookmarked (because
  `CAP_URL_DISPLAY ≤ CAP_BOOKMARK_URL`).
- A URL that was bookmarked but is longer than `CAP_URL_DISPLAY` cannot be re-displayed as text
  on the ShowUrl screen; it can only be shown as a QR code from the bookmark list.

### Where the caps live

Both constants are defined in `dc34-vault/src/capacity.rs` (where the measured display and QR
capacity values live, alongside documentation of their measurement method) and re-exported from
`dc34-vault/src/sanitize.rs` so callers have a single import path.

---

## 2. HID Type-Out Contract

### Entry point

`send_str_sanitized(&SanitizedUrl)` is the only legal entry point for URL type-out over USB HID.
It is implemented in `dc34-vault/src/sanitize.rs` and must never be bypassed by callers that hold
a raw `&str`.

The existing password autotype path (`pwauth://`) uses a separate `send_str` call in `usb-bao1x`
and is not affected by this function. New URL type-out always goes through `send_str_sanitized`.

### Why no stripping is needed at type-out time

`SanitizedUrl`'s constructor already checked every character against the HID map and rejected the
string if any character was unmapped. By the time `send_str_sanitized` is called, it is a
compile-time guarantee (enforced by the type system) that every character in the payload has a
known HID code. The function emits HID keycodes directly without any per-character guard.

This is the primary reason `SanitizedUrl` exists as a type rather than a runtime flag: the check
happens once at the trust boundary (QR scan) and the type carries the proof forward.

### What the confirmation modal guarantees

The ShowUrl confirmation modal displays `SanitizedUrl.as_str()` — the same bytes that
`send_str_sanitized` will emit. Because `SanitizedUrl`'s value is immutable after construction,
and because `send_str_sanitized` operates directly on those bytes, the displayed string and the
typed string are identical by construction. There is no post-display sanitization step that could
diverge them.

A URL containing characters that would be typed differently from how they appear (e.g. emoji,
look-alike Unicode, unmapped glyphs) never reaches the confirmation screen: the constructor rejects
it at scan time.

### UseBeforeInit error

`send_str_sanitized` returns `Err(UseBeforeInit)` when the USB HID connection has not yet been
established (the USB subsystem is initialized lazily). The call site in `dc34-vault` handles this
by displaying a brief error notification and returning to `VaultMode::ShowUrl` so the user can
retry. No bytes are typed on a `UseBeforeInit` return; the modal is not re-shown automatically.

### Security note: this is keystroke injection

URL type-out is keystroke injection from an untrusted source. A malicious QR code could contain a
URL that, when typed to a connected host, triggers clipboard paste sequences, terminal escape
codes, or shell metacharacters. The security controls are:

1. **`SanitizedUrl` type** — rejects any character that is not in the plain US-101 HID mapping,
   and rejects control characters (`\x00`–`\x1F`, `\x7F`). This does NOT remove shell
   metacharacters: `&`, `;`, `|`, `$`, `` ` ``, `>`, `<`, `!`, `(`, `)` are all valid US-101
   keys and will be typed verbatim if present in a URL that passes the constructor.
2. **User-presence confirmation** — the user must explicitly press a button to confirm type-out
   after seeing the exact bytes that will be typed. There is no auto-type on scan.

Neither control prevents a syntactically valid URL from being a phishing link or containing a
path that an application on the host will interpret dangerously. The user is responsible for
reading the confirmation screen.

---

## 3. vault.bookmarks PDDB Schema

### Dict name

```rust
pub const VAULT_BOOKMARKS_DICT: &str = "vault.bookmarks";
```

Defined alongside `VAULT_PASSWORD_DICT` and `VAULT_TOTP_DICT` in `dc34-vault/src/vault_api.rs`.

### Key format

Each bookmark is stored under a zero-padded, 16-character hexadecimal representation of a `u64`
monotonic counter. Examples:

```
0000000000000001
0000000000000002
000000000000000f
0000000000000010
```

Keys sort lexicographically in insertion order. Keys are never reused: after a delete, the counter
continues from its last value. The key for the next insert is strictly greater than any key that
has ever existed in the dict, including deleted ones.

### Counter key

The counter value is persisted under the reserved key `"__counter__"` in the same dict.
Read-modify-write is performed under a single PDDB IPC call. The counter starts at 0; the first
bookmark written receives key `0000000000000001`.

Any code that enumerates bookmark records by iterating dict keys must skip keys that do not match
the 16-character hex format — specifically `"__counter__"` and any future reserved keys.

### Record body

Each bookmark value is a newline-delimited UTF-8 string:

```
<url>\n<label>\n<timestamp_unix>
```

- `url` — the full URL, a valid `SanitizedUrl` value (no trailing newline in the field itself).
- `label` — a human-readable name for the bookmark; may be empty; no embedded newlines.
- `timestamp_unix` — decimal representation of a Unix timestamp (seconds since epoch) as recorded
  by the Xous RTC at bookmark creation time.

Parsing splits on `\n` and takes the first three fields. Extra fields (future extensibility) are
ignored. Missing fields return `Err` from the deserializer.

### Basis

Bookmarks are stored in the **default basis**. The default basis is always unlocked when the vault
app is running; no additional unlock step is required.

**Locked-basis behavior:** If the PDDB basis is locked at the moment of a write (which should not
happen under normal operation but is possible during a PDDB stress state), `pddb_store` returns
`Err`. The bookmark layer propagates this error to the caller without advancing the counter and
without performing a partial write. The user sees an error notification; no data is corrupted.

### Limits

- **Maximum 100 bookmarks.** Enforced at ingest time: the caller queries the current bookmark
  count from PDDB and passes it to `SanitizedUrl::new` (or a wrapper around it in `sanitize.rs`);
  if the count is already 100, the constructor returns `Err` before any PDDB write occurs. This
  check happens in the presentation layer, not in the storage layer.
- **Maximum URL length = `CAP_BOOKMARK_URL` bytes.** Enforced by `SanitizedUrl`'s constructor
  (see section 1). A URL longer than this cap is rejected at scan time with `Err`.

### Namespace isolation

Bookmark code opens only `"vault.bookmarks"`. It never opens `"dc34"`, `"vault.passwords"`, or
`"vault.totp"`. The constants `DC34_DICT`, `DC34_GENE`, and `DC34_SECRET` from `dc34-api` are not
imported into any file that performs bookmark operations. `save_light_gene()` is not reachable
from any bookmark code path.

---

## 4. URL-Before-Base45 Dispatch Ordering

### Why the ordering matters

The DC34 gene exchange protocol encodes binary payloads as base45 strings. The base45 alphabet
includes all uppercase ASCII letters (`A`–`Z`), digits, and a small set of punctuation. A URL
such as `HTTPS://EXAMPLE.COM/PATH` is entirely valid base45 and would be silently misdecoded as
binary gene data if base45 decoding were attempted first.

The base45 decoder does not fail gracefully on non-gene payloads: it decodes to arbitrary bytes,
the gene protocol header check (`DC34_HEADER`) fails, and the payload is discarded. The user sees
no URL.

### Where the ordering is enforced

Two locations in dc34-vault enforce the URL-first rule:

**`dc34-vault/src/main.rs` — `VaultOp::HandleQr` arm**

This is the top-level dispatcher. When a QR scan result arrives in `VaultMode::Idle` or
`VaultMode::IdleDevMode`, the code checks whether the decoded string starts with `http://` or
`https://` before attempting any base45 decode. If the prefix matches, the string is passed to
`SanitizedUrl::new` and, on success, the mode is set to `VaultMode::ShowUrl`. Base45 decode is
never attempted for that payload.

Gene-protocol modes (`GeneScan`, `ResponseGene`, `ShowKey`) are matched before the URL check;
those modes feed their payloads directly to base45 without a URL prefix test.

**`dc34-vault/src/actions.rs` — match arm in `ActionOp::HandleQr`**

The action handler mirrors the same precedence. The URL scheme check is the first arm inside the
QR handler; the existing `_ => qr_unrecognized` fallback arm catches anything that is neither a
URL nor a recognized scheme.

### What the ordering is, explicitly

For a QR scan arriving in `VaultMode::Idle`:

1. Check if the decoded string starts with `http://` or `https://`.
   - Yes → attempt `SanitizedUrl::new`. On success, enter `VaultMode::ShowUrl`. Done.
   - `SanitizedUrl::new` failure → show error, return to Idle. Base45 decode is NOT attempted.
2. Check known non-URL schemes: `otpauth://`, `pwauth://`, `time://`, `search://`, `test://`,
   `factory://`, `dc34://`, `gene://`. Each dispatches to its existing handler unchanged.
3. Attempt `base45::decode`.
   - Success with a valid DC34 header → gene exchange path.
   - Otherwise → `qr_unrecognized` error display.

The URL check is step 1. It cannot be reordered without breaking `HTTPS://`-style URLs that are
coincidentally valid base45.

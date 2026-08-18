//! URL sanitization and HID character boundary enforcement.
//!
//! SanitizedUrl is the mandatory type for all URL data crossing task boundaries.
//! Any URL that enters ShowUrl mode or gets typed via HID MUST be a SanitizedUrl.
//!
//! char_to_hid_code_us101 in xous-core/services/usb-bao1x/src/mappings.rs is NOT
//! reachable from dc34-vault under the board-baosec build (see SYMBOLS.md Q2).
//! The accepted-char set is therefore defined locally here, with a test asserting
//! that every character in HID_ACCEPTED is printable ASCII (the upstream table covers
//! the full printable ASCII range 0x20-0x7E under board-baosec).

/// Maximum URL length for on-screen text display.
/// TEXT_COLS(21) x (TEXT_ROWS-1)(9) = 189. PROVISIONAL - update after Task 3
/// ShowUrl rendering confirms actual column width (may be 19 if LEFT_MARGIN applies).
pub const CAP_URL_DISPLAY: usize = 189;

/// Maximum URL length for QR rendering and bookmark storage.
/// Derived from QR V=6/EcLevel::M byte capacity: (864-16)/8 = 106 bytes. PROVISIONAL.
pub const CAP_BOOKMARK_URL: usize = 106;

/// A URL string that has been validated against the HID character set and a length cap.
///
/// Invariants guaranteed by the constructor:
///   - Every byte is printable ASCII (0x20-0x7E, no control chars, no non-ASCII)
///   - Byte length <= the cap passed to `new`
///   - The type is never constructed by truncation -- only rejection
#[derive(Debug, Clone)]
pub struct SanitizedUrl(String);

/// Errors returned by `SanitizedUrl::new`.
#[derive(Debug)]
pub enum SanitizeError {
    /// A control character (< 0x20) was found, including \n, \r, \t, BEL, NUL.
    ControlChar(char),
    /// A character not in the accepted HID printable-ASCII set was found.
    UnmappedChar(char),
    /// The string byte-length exceeds the supplied cap.
    TooLong { len: usize, cap: usize },
}

/// Accepted HID characters -- full printable ASCII (0x20-0x7E).
///
/// This set is a local copy of the characters that char_to_hid_code_us101 maps
/// under the precursor/bao1x features of usb-bao1x. Since that function is not
/// reachable from dc34-vault under board-baosec (SYMBOLS.md Q2), we define it here.
/// The test `hid_accepted_is_full_printable_ascii` asserts structural correctness.
const HID_ACCEPTED: &str =
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

impl SanitizedUrl {
    /// Fallible constructor. Returns `Err` -- never truncates -- on:
    ///   - Any control character (< 0x20), including \n, \r, \t, BEL, NUL
    ///   - Any character not in the accepted HID printable-ASCII set (e.g. DEL, emoji)
    ///   - Byte length exceeding `cap`
    pub fn new(s: &str, cap: usize) -> Result<Self, SanitizeError> {
        if s.len() > cap {
            return Err(SanitizeError::TooLong { len: s.len(), cap });
        }
        for ch in s.chars() {
            if (ch as u32) < 0x20 {
                return Err(SanitizeError::ControlChar(ch));
            }
            if !HID_ACCEPTED.contains(ch) {
                return Err(SanitizeError::UnmappedChar(ch));
            }
        }
        Ok(SanitizedUrl(s.to_string()))
    }

    /// Returns the validated URL as a string slice.
    pub fn as_str(&self) -> &str { &self.0 }

}

impl AsRef<str> for SanitizedUrl {
    fn as_ref(&self) -> &str { self.as_str() }
}

/// Route all URL HID type-out through this function (Task 5 entry point).
///
/// Takes a `SanitizedUrl` so by construction all characters are mapped to HID keycodes
/// and no control characters exist. Do not add new UsbHid call sites -- route through
/// this wrapper.
pub fn send_str_sanitized(usb: &usb_bao1x::UsbHid, url: &SanitizedUrl) -> Result<usize, xous::Error> {
    usb.send_str(url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_newline() {
        let result = SanitizedUrl::new("http://a.com/\n", CAP_URL_DISPLAY);
        assert!(result.is_err(), "newline should be rejected");
    }

    #[test]
    fn rejects_bel() {
        let result = SanitizedUrl::new("http://a.com/\u{7}", CAP_URL_DISPLAY);
        assert!(result.is_err(), "BEL (U+0007) should be rejected");
    }

    #[test]
    fn rejects_emoji() {
        // crab emoji = U+1F980
        let result = SanitizedUrl::new("http://a.com/\u{1F980}", CAP_URL_DISPLAY);
        assert!(result.is_err(), "emoji should be rejected");
    }

    #[test]
    fn rejects_overlength() {
        let s = "A".repeat(CAP_URL_DISPLAY + 1);
        let result = SanitizedUrl::new(&s, CAP_URL_DISPLAY);
        assert!(result.is_err(), "string of CAP_URL_DISPLAY+1 chars should be rejected");
    }

    #[test]
    fn accepts_valid_url() {
        let result = SanitizedUrl::new("https://example.com/x?a=b&c=d#frag", CAP_URL_DISPLAY);
        assert!(result.is_ok(), "valid URL should be accepted: {:?}", result);
    }

    #[test]
    fn rejection_is_err_not_truncated() {
        let s = "B".repeat(CAP_URL_DISPLAY + 1);
        match SanitizedUrl::new(&s, CAP_URL_DISPLAY) {
            Err(_) => {} // correct -- rejection, not truncation
            Ok(url) => panic!("Expected Err, got Ok with len={}", url.len()),
        }
    }

    #[test]
    fn hid_accepted_is_full_printable_ascii() {
        // Assert that HID_ACCEPTED contains exactly the 95 printable ASCII chars (0x20-0x7E).
        assert_eq!(HID_ACCEPTED.len(), 95, "HID_ACCEPTED should have 95 printable ASCII chars");
        for c in 0x20u8..=0x7Eu8 {
            let ch = c as char;
            assert!(
                HID_ACCEPTED.contains(ch),
                "HID_ACCEPTED missing printable ASCII char 0x{:02X} ({})",
                c,
                ch
            );
        }
        // No char outside 0x20-0x7E should appear in HID_ACCEPTED
        for ch in HID_ACCEPTED.chars() {
            let v = ch as u32;
            assert!(
                v >= 0x20 && v <= 0x7E,
                "HID_ACCEPTED contains out-of-range char U+{:04X}",
                v
            );
        }
    }

    #[test]
    fn rejects_tab() {
        assert!(SanitizedUrl::new("http://a.com/\t", CAP_URL_DISPLAY).is_err());
    }

    #[test]
    fn rejects_carriage_return() {
        assert!(SanitizedUrl::new("http://a.com/\r", CAP_URL_DISPLAY).is_err());
    }

    #[test]
    fn rejects_del() {
        // DEL (0x7F) is not in HID_ACCEPTED
        assert!(SanitizedUrl::new("http://a.com/\x7f", CAP_URL_DISPLAY).is_err());
    }

    #[test]
    fn accepts_at_exact_cap() {
        let s = "A".repeat(CAP_URL_DISPLAY);
        assert!(SanitizedUrl::new(&s, CAP_URL_DISPLAY).is_ok());
    }
}

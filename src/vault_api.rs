use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use locales::t;
use num_traits::*;

// This file contains items that are used simultaneously within OpenSK and the `vault` app itself.
// These items need to be pulled in via both `lib` and `main` scopes.
// Vault-specific command to upload TOTP codes
pub const COMMAND_RESTORE_TOTP_CODES: u8 = 0x71;
pub const COMMAND_BACKUP_TOTP_CODES: u8 = 0x72;
pub const COMMAND_RESET_SESSION: u8 = 0x74;

pub const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";
pub const VAULT_TOTP_DICT: &'static str = "vault.totp";
pub const VAULT_BOOKMARKS_DICT: &'static str = "vault.bookmarks";
/// FIDO2 / U2F registrations, surfaced to the user as PASSKEYS.
/// Declared here because this module is pulled into both the lib and bin scopes, which is
/// the only place `env::xous` and `storage` can share a definition.
pub const U2F_APP_DICT: &'static str = "fido.u2fapps";
pub const VAULT_BOOKMARKS_COUNTER_KEY: &'static str = "__counter__";
/// contains the list of usernames
pub const VAULT_CONFIG_USERNAMES: &'static str = "vault.config/usernames";
/// contains the generator configuration record
pub const VAULT_CONFIG_GENERATOR: &'static str = "vault.config/generator";

/// bytes to reserve for a key entry. Making this slightly larger saves on some churn as stuff gets updated
pub const VAULT_ALLOC_HINT: usize = 256;

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum VaultOp {
    /// Redraw the screen
    Redraw = 0,
    ReloadDbAndFullRedraw,
    KeyPress,

    /// Main menu
    MenuChangeFont,
    MenuDeleteStage1,
    MenuEditStage1,
    MenuAutotype,
    MenuReadoutMode,
    MenuAutotypeRate,
    MenuLeftyMode,
    MenuDone,
    BadgeMode,
    MenuTokenHelp,
    MenuUsernames,
    MenuFilter,

    /// Tour menu
    TourContinue,
    TourLater,
    TourNever,

    /// Gene menu
    KeepGene,
    RevertGene,

    /// Badge menu
    DefconHelp,
    About,
    TokenMode,
    PowerOff,
    ScreenOff,

    BasisChange,

    // for QR responses not handled by the action manager
    HandleQr,
    AbortQr,

    // bookmark QR: ActionManager validated the URL; main builds the QrCode and sets qr_override
    BookmarkQrReady,

    // open bookmark list screen: main loads bookmarks via vault_ui then sets VaultMode::BookmarkList
    ListBookmarks,

    // monkey patch for last-minute custom image feature - discriminant is hard-coded into dc34-console
    ImageLoad = 1024,
    // monkey patch to force jig mode, for re-tested units in the factory
    Jig = 1025,
    // monkey patch to skip the first key after a WFI event
    SkipKey = 1026,
    // monkey patch to indicate if BIO hacks are active
    BioActive = 1027,
    // menu-initiated QR scan. The menu widget can only send non-blocking scalars, so it
    // cannot invoke ActionOp::AcquireQr (a blocking scalar) directly. Main does the camera
    // setup and issues the blocking call on the menu's behalf.
    //
    // Explicitly numbered, deliberately: taking the next free auto-discriminant would claim
    // a low opcode that other components may already emit, silently turning a previously
    // ignored message into a live camera-acquisition call.
    ScanUrl = 1028,

    // S-CAM menu targets. Explicitly numbered from 1029 for the same reason ScanUrl is:
    // taking the next free auto-discriminant would claim a low opcode that other
    // components may already emit.
    /// stored passwords
    ListPasswords = 1029,
    /// TOTP entries, shown to the user as "2FA DIGITS"
    List2faDigits = 1030,
    /// FIDO2 credentials, shown to the user as "PASSKEYS" - a screen that does not exist today
    ListPasskeys = 1031,
    /// captured photos
    ListPhotos = 1032,
    /// display image selection
    SettingsBling = 1033,
    /// LED pattern selection
    SettingsBlinky = 1034,
    /// QR of the repo README
    ShowAbout = 1036,
    /// reopen the top-level menu, e.g. from a submenu's "back"
    MenuRoot = 1037,
    /// open the login-details submenu
    MenuLogin = 1038,
    /// open the settings submenu
    MenuSettings = 1039,
    /// the menu widget reporting that it closed itself, on select or on LEFT. Distinct from
    /// MenuDone, which is the "back" entry asking to leave the menu tree entirely.
    MenuClosed = 1040,
}

// Compile-time guard on the VaultOp wire contract.
//
// These discriminants cross service boundaries and are invisible to the type system:
//   * `KeyPress` is handed to the graphics service by value via `gfx.register_listener()`
//     (see main.rs) -- the graphics service sends this number back on every key event.
//   * `SkipKey` (1026) is written as a bare literal in dc34-console/src/power.rs.
// Inserting a variant *above* any of these silently renumbers them: the code still
// compiles, but key input or power handling breaks at runtime. Pin them here so that
// mistake is a build failure instead of a field debugging session.
const _: () = assert!(VaultOp::Redraw as isize == 0);
const _: () = assert!(VaultOp::KeyPress as isize == 2);
const _: () = assert!(VaultOp::ImageLoad as isize == 1024);
const _: () = assert!(VaultOp::Jig as isize == 1025);
const _: () = assert!(VaultOp::SkipKey as isize == 1026);
const _: () = assert!(VaultOp::BioActive as isize == 1027);
// The auto-numbered block must never grow into the hard-coded 1024+ range.
const _: () = assert!((VaultOp::ListBookmarks as isize) < 1024);
const _: () = assert!(VaultOp::ScanUrl as isize == 1028);
const _: () = assert!(VaultOp::ListPasswords as isize == 1029);
const _: () = assert!(VaultOp::List2faDigits as isize == 1030);
const _: () = assert!(VaultOp::ListPasskeys as isize == 1031);
const _: () = assert!(VaultOp::ListPhotos as isize == 1032);
const _: () = assert!(VaultOp::SettingsBling as isize == 1033);
const _: () = assert!(VaultOp::SettingsBlinky as isize == 1034);
const _: () = assert!(VaultOp::ShowAbout as isize == 1036);
const _: () = assert!(VaultOp::MenuRoot as isize == 1037);
const _: () = assert!(VaultOp::MenuLogin as isize == 1038);
const _: () = assert!(VaultOp::MenuSettings as isize == 1039);
const _: () = assert!(VaultOp::MenuClosed as isize == 1040);

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct IpcString {
    pub s: String,
}

pub fn atime_to_str(req_atime: u64) -> String {
    let mut request_str = String::with_capacity(
        // avoid allocations to speed up this routine, it is in the inner loop of rendering lists of
        // passwords
        t!("vault.u2f.appinfo.last_authtime", locales::LANG).len()
            + t!("vault.u2f.appinfo.seconds_ago", locales::LANG).len()
            + 16, // space for the actual duration + some slop for translation
    );
    if req_atime == 0 {
        request_str.push_str(t!("vault.u2f.appinfo.last_authtime", locales::LANG));
        request_str.push_str(t!("vault.u2f.appinfo.never", locales::LANG));
    } else {
        let now = utc_now();
        let atime = DateTime::<Utc>::from_timestamp(req_atime as i64, 0).unwrap_or_default();
        // avoid format! macro, it is too slow.
        if now.signed_duration_since(atime).num_days() > 1 {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", locales::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_days().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.days_ago", locales::LANG));
        } else if now.signed_duration_since(atime).num_hours() > 1 {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", locales::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_hours().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.hours_ago", locales::LANG));
        } else if now.signed_duration_since(atime).num_minutes() > 1 {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", locales::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_minutes().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.minutes_ago", locales::LANG));
        } else {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", locales::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_seconds().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.seconds_ago", locales::LANG));
        }
    }
    request_str
}

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid
/// target
pub fn utc_now() -> DateTime<Utc> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
    DateTime::<Utc>::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32).unwrap_or_default()
}

/// app info format:
///
/// name: free form text string until newline
/// hash: app hash in hex string, lowercase
/// created: decimal number representing epoch of the creation date
/// last auth: decimal number representing epoch of the last auth time
pub struct AppInfo {
    pub name: String,
    pub id: [u8; 32],
    pub notes: String,
    pub ctime: u64,
    pub atime: u64,
    pub count: u64,
}

pub fn deserialize_app_info(descriptor: Vec<u8>) -> Option<AppInfo> {
    if let Ok(desc_str) = String::from_utf8(descriptor) {
        let mut appinfo = AppInfo {
            name: String::new(),
            notes: String::new(),
            id: [0u8; 32],
            ctime: 0,
            atime: 0,
            count: 0,
        };
        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "name" => {
                        appinfo.name.push_str(data);
                    }
                    "notes" => appinfo.notes.push_str(data),
                    "id" => {
                        if let Ok(id) = hex::decode(data) {
                            appinfo.id.copy_from_slice(&id);
                        } else {
                            return None;
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            appinfo.ctime = ctime;
                        } else {
                            return None;
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            appinfo.atime = atime;
                        } else {
                            return None;
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            appinfo.count = count;
                        }
                        // count was added later, so, we don't fail if we don't see the record.
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing app info, aborting", tag);
                        return None;
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        if appinfo.name.len() > 0 && appinfo.id != [0u8; 32] {
            // atime can be 0 - indicates never used. In hosted mode, ctime is 0.
            Some(appinfo)
        } else {
            None
        }
    } else {
        None
    }
}

pub fn serialize_app_info<'a>(appinfo: &AppInfo) -> Vec<u8> {
    format!(
        "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
        "name",
        appinfo.name,
        "id",
        hex::encode(appinfo.id),
        "ctime",
        appinfo.ctime,
        "atime",
        appinfo.atime,
        "count",
        appinfo.count,
    )
    .into_bytes()
}

pub fn basis_change() {
    log::info!("got basis change");
    xous::send_message(
        SELF_CONN.load(core::sync::atomic::Ordering::SeqCst),
        xous::Message::new_scalar(VaultOp::BasisChange.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .unwrap();
}
pub static SELF_CONN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

use num_traits::*;
use ux_api::menu::*;

use crate::VaultOp;

/// The S-CAM menu tree.
///
/// Three menus, one builder. The tree is two levels deep: the credential screens sit under
/// "login deets" and the device settings under "settings", so the top level stays short
/// enough to read on a 128px panel without scrolling past the useful entries.
///
/// Every entry closes its menu on select. Actions arrive at the main loop as NON-blocking
/// scalars (see ux-api menu.rs), so each opcode below must be handled that way and must
/// clear `menu_active` - `tools/check-menu-wiring.py` fails the build otherwise.
fn build(
    title: &'static str,
    entries: &[(&str, VaultOp)],
    vault_conn: xous::CID,
    menu_mgr: xous::SID,
) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();
    for (name, op) in entries {
        menu_items.push(MenuItem {
            name: String::from(*name),
            action_conn: Some(vault_conn),
            action_opcode: op.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });
    }
    // MenuClosed, not MenuDone. The widget sends this to the parent whenever it closes -
    // after every selection, and on LEFT - so it cannot be the same opcode as the "back"
    // entry that tears the whole tree down. Wiring them together meant selecting a submenu
    // drew it and then immediately closed it back to the idle screen.
    menu_matic(menu_items, title, Some(menu_mgr), vault_conn, VaultOp::MenuClosed.to_usize().unwrap())
        .expect("couldn't create MenuMatic manager")
}

/// Top level. "back" closes the menu and returns to standby; it maps to MenuDone rather than
/// PowerOff, because "screen off" is its own entry under settings and two shutdown-sounding
/// paths would be ambiguous.
pub fn create_root(vault_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    build(
        crate::theme::BADGE_NAME,
        &[
            ("login deets", VaultOp::MenuLogin),
            // the badge's saved URLs, which only ever arrive by scanning a QR code
            ("qr collection", VaultOp::ListBookmarks),
            ("photos", VaultOp::ListPhotos),
            ("settings", VaultOp::MenuSettings),
            ("about", VaultOp::ShowAbout),
            ("back", VaultOp::MenuDone),
        ],
        vault_conn,
        menu_mgr,
    )
}

/// The three credential stores. They are separate screens because they are separate stores:
/// a passkey is not a password, and the old tree named only two of the three.
pub fn create_login(vault_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    build(
        "LOGIN DEETS",
        &[
            ("2fa digits", VaultOp::List2faDigits),
            ("passkeys", VaultOp::ListPasskeys),
            ("passwords", VaultOp::ListPasswords),
            ("back", VaultOp::MenuRoot),
        ],
        vault_conn,
        menu_mgr,
    )
}

pub fn create_settings(vault_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    build(
        "SETTINGS",
        &[
            ("bling", VaultOp::SettingsBling),
            ("blinky", VaultOp::SettingsBlinky),
            ("screen off", VaultOp::ScreenOff),
            ("back", VaultOp::MenuRoot),
        ],
        vault_conn,
        menu_mgr,
    )
}

/// Actions on the photo under the cursor.
///
/// Built through the same helper as every other menu, so it looks and behaves like the

/// Yes/no confirmation, as a menu rather than a modal.
///
/// The modal widget ignores LEFT and RIGHT outright and draws no button labels, so a
/// confirmation asked that way had no way to back out and no hint about the controls. Built
/// here it behaves like every other list: LEFT backs out, the same brackets mark the focus.
///

/// Like `build`, but each entry carries a value in its scalar payload. Used where the entries
/// are the same action at different settings, so they share one opcode instead of needing one
/// each.
fn build_payload(
    title: &'static str,
    entries: &[(&str, VaultOp, u32)],
    vault_conn: xous::CID,
    menu_mgr: xous::SID,
) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();
    for (name, op, value) in entries {
        menu_items.push(MenuItem {
            name: String::from(*name),
            action_conn: Some(vault_conn),
            action_opcode: op.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([*value, 0, 0, 0]),
            close_on_select: true,
        });
    }
    menu_matic(menu_items, title, Some(menu_mgr), vault_conn, VaultOp::MenuClosed.to_usize().unwrap())
        .expect("couldn't create MenuMatic manager")
}

/// Typing speed test. Types the alphabet at a chosen delay so the speed at which the host

/// Entries for the one reusable menu, by purpose.
///
/// These three were a menu each. Every `menu_matic` costs a server and TWO threads, and three
/// of them made the badge noticeably less stable - it is a swap-resident app with a real
/// memory budget. One manager, retitled and refilled before it opens, costs a fraction of that.
pub const PHOTO_ACTIONS: [(&str, VaultOp, u32); 5] = [
    ("set wallpaper", VaultOp::PhotoSetWallpaper, 0),
    ("export b64", VaultOp::PhotoExportB64, 0),
    ("export ascii", VaultOp::PhotoExportAscii, 0),
    ("delete", VaultOp::PhotoDelete, 0),
    ("back", VaultOp::MenuClosed, 0),
];

/// "no" first, so the default selection is the harmless one.
pub const CONFIRM: [(&str, VaultOp, u32); 2] =
    [("no", VaultOp::ConfirmNo, 0), ("yes", VaultOp::ConfirmYes, 0)];

/// The reusable menu. Created empty; `fill` gives it a title and entries before each use.
pub fn create_scratch(vault_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    build_payload("", &[], vault_conn, menu_mgr)
}

/// Retitle and refill the reusable menu, returning the labels now in it so the next fill can
/// clear them again.
pub fn fill(
    menu: &MenuMatic,
    previous: &[String],
    title: &str,
    entries: &[(&str, VaultOp, u32)],
    vault_conn: xous::CID,
) -> Vec<String> {
    for name in previous {
        menu.delete_item(name);
    }
    for (label, op, value) in entries {
        menu.add_item(MenuItem {
            name: String::from(*label),
            action_conn: Some(vault_conn),
            action_opcode: op.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([*value, 0, 0, 0]),
            close_on_select: true,
        });
    }
    menu.set_title(title);
    menu.set_index(0);
    entries.iter().map(|(l, _, _)| String::from(*l)).collect()
}

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
    menu_matic(menu_items, title, Some(menu_mgr), vault_conn, VaultOp::MenuDone.to_usize().unwrap())
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

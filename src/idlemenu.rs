use num_traits::*;
use ux_api::menu::*;

use crate::VaultOp;

/// The S-CAM menu. Each entry maps to exactly one store, which is the point: the badge holds
/// three kinds of credential and the old tree named only two of them, leaving FIDO2
/// credentials stored but invisible. See DECISIONS.md.
pub fn create_submenu(vault_conn: xous::CID, actions_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();

    // (label, opcode, connection)
    let entries: [(&str, u32, xous::CID); 10] = [
        ("passwords",     VaultOp::ListPasswords.to_u32().unwrap(),  vault_conn),
        ("2fa digits",    VaultOp::List2faDigits.to_u32().unwrap(),  vault_conn),
        ("passkeys",      VaultOp::ListPasskeys.to_u32().unwrap(),   vault_conn),
        ("bookmarks",     VaultOp::ListBookmarks.to_u32().unwrap(),  vault_conn),
        ("photos",        VaultOp::ListPhotos.to_u32().unwrap(),     vault_conn),
        // The three settings sit inline rather than behind a "settings" entry. A submenu
        // needs a second MenuMatic and its own back handling for three items, and the
        // single "settings" entry it replaced went straight to standby image anyway, so
        // LED pattern and screen off had no route from the menu at all.
        ("standby image", VaultOp::SettingsBling.to_u32().unwrap(),  vault_conn),
        ("led pattern",   VaultOp::SettingsBlinky.to_u32().unwrap(), vault_conn),
        ("screen off",    VaultOp::ScreenOff.to_u32().unwrap(),      vault_conn),
        ("about",         VaultOp::ShowAbout.to_u32().unwrap(),      vault_conn),
        // EXIT closes the menu and returns to standby. It maps to MenuDone, not PowerOff -
        // "screen off" is its own entry and two shutdown-sounding paths would confuse.
        ("exit",          VaultOp::MenuDone.to_u32().unwrap(),       vault_conn),
    ];

    for (name, opcode, conn) in entries {
        menu_items.push(MenuItem {
            name: String::from(name),
            action_conn: Some(conn),
            action_opcode: opcode,
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });
    }

    // Menu actions are delivered as NON-blocking scalars (ux-api menu.rs), so every opcode
    // above must be handled that way in the main loop. Routing one at a blocking-scalar
    // handler is why "Scan URL" silently did nothing.
    let _ = actions_conn;

    menu_matic(menu_items, crate::theme::BADGE_NAME, Some(menu_mgr), vault_conn, VaultOp::MenuDone.to_usize().unwrap())
        .expect("couldn't create MenuMatic manager")
}

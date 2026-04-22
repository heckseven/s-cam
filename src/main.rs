mod ux;
use aes::{Aes256, cipher::BlockSizeUser};
use aes_gcm_siv::aead::{Aead, Payload};
use aes_gcm_siv::{Nonce, Tag};
use ux::*;
mod itemcache;
use itemcache::*;
mod actions;
mod storage;
mod submenu;
mod totp;
mod tourmenu;
pub mod vault_api;
use ux_api::service::gfx::Gfx;
pub use vault_api::*;
mod action_handler;
mod bitmaps;
mod config;
mod fido2;
mod genemenu;
mod generator;
mod idlemenu;
mod tests;
mod vendor_commands;

use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dc34_api::*;
use locales::t;
use num_traits::*;
use pddb::Pddb;
use qrcode::QrCode;
use xous_ipc::Buffer;

use crate::actions::ActionOp;
use crate::config::GlobalConfig;

/*
To do:

- Add user logo
    - [ ] upload of data via base64 over serial
    - [ ] animation sequence
    - [ ] menu item to delete user logo
- Tour improvements
    - [x] force orientation to be right side up for tour
    - [x] change away from 'breeding' language -> mix? remix?
    - [x] put the defon.org url in the tour (get final URL from jeff week of 4/9)
- Menu has Edit / Delete / Usernames / About / Help / Close
    - [x] Filter -> if any entries, add filter string entry
    - [x] Usernames brings up list of usernames. If empty, prompt to enter new username.
    - [x] Edit edits the current entry, if any
    - [x] Delete deletes the current entry, if any
    - [x] Help shows "Help" sequence in token mode
    - [x] About shows about sequence
    - [-] [optional - medium priority] PIN code -> activates PIN menu
    - [-] [optional - lowest priority] Backups
- Bugs
    - [ ] wear badge and confirm that deep sleep works as expected
    - [ ] stability on power management - more testing
    - [?] fix battery voltage reading (maybe just an issue on old badges???)
    - [?] fix glitch on WFI transition
    - [x] tune accelerometer settings
    - [x] deep sleep after X time
    - [x] always auto-boot on warm boot - no bypass on keypress
    - [x] add battery voltage printing on screen in about screen
    - [x] power-off on vbat low - low battery screen
    - [x] Idle after X time
    - [x] disable idle on Vbus detect
    - [x] Wake up on key press or accelerometer
- Factory improvements
    - [x] on first badge mate, show note indicating badge initialization, instead of "mismatch" error
    - [x] on token mode, don't show "mismatch" error
    - [x] add diagnostics to "about" screen
- Experience
    - [x] flashing eyes when upside down
- Stability testing - especially in token mode
    - [x] "use" bug when not plugged into USB after enrolling a credential (system hangs)
    - [x] totp redraw once too many on toggle to pw mode
    - [x] no passwords present on initial entry to pw mode
    - CI setup with opensk tester
    - [-] small flourish: resume from deep sleep into previous vault state

*/

pub(crate) const SERVER_NAME_VAULT2: &str = "_Vault2_";

#[derive(Copy, Clone, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VaultMode {
    Idle,
    IdleDevMode,
    ShowKey { quantum: u32 },
    ResponseGene { quantum: u32 },
    // state for confirming the current pattern
    ConfirmGene,
    GeneScan,
    FactoryTest,
    StandAloneTest,
    Tour,
    TokenTour,
    DefconHelp,
    About,
    Totp,
    Password,
    TokenHelp,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("dc34-vault PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let gfx = Gfx::new(&xns).unwrap();
    gfx.clear().ok();
    // show the DC logo
    gfx.bitmap(&bitmaps::dc_logo::BITMAP, None, None).ok();
    gfx.flush().ok();
    let tt = ticktimer_server::Ticktimer::new().unwrap();

    // Register the server with xous
    let sid = xns.register_name(SERVER_NAME_VAULT2, None).expect("can't register server");
    let conn = xous::connect(sid).unwrap();

    // global shared state
    let mode = Arc::new(Mutex::new(VaultMode::Idle));
    let animate = Arc::new(AtomicBool::new(true));
    let item_lists = Arc::new(Mutex::new(ItemLists::new()));
    let action_active = Arc::new(AtomicBool::new(false));
    // Protects access to the openSK PDDB entries from simultaneous readout on the UX while OpenSK is updating
    let opensk_mutex = Arc::new(Mutex::new(0));
    let allow_host = Arc::new(AtomicBool::new(false));

    // spawn the TOTP pumper
    let pump_sid = xous::create_server().unwrap();
    crate::totp::pumper(pump_sid, conn, animate.clone());
    let pump_conn = xous::connect(pump_sid).unwrap();

    // respond to keyboard events - register with the `Gfx` subsystem, so we're getting keypresses
    // filtered by the modals interface
    gfx.register_listener(SERVER_NAME_VAULT2, VaultOp::KeyPress.to_u32().unwrap() as usize);

    // spawn the actions server. This is responsible for grooming the UX elements. It
    // has to be in its own thread because it uses blocking modal calls that would cause
    // redraws of the background list to block/fail.
    let actions_sid = xous::create_server().unwrap();
    let actions_conn = xous::connect(actions_sid).unwrap();

    let mut vault_ui = VaultUi::new(
        &xns,
        conn.clone(),
        item_lists.clone(),
        mode.clone(),
        animate.clone(),
        pump_conn,
        actions_conn,
    );

    action_handler::action_handler(
        conn.clone(),
        actions_sid,
        mode.clone(),
        item_lists.clone(),
        action_active.clone(),
    );

    fido2::fido2_handler(conn, allow_host.clone(), opensk_mutex.clone());

    let menu_sid = xous::create_server().unwrap();
    let menu_mgr = submenu::create_submenu(conn, actions_conn, menu_sid);
    let tour_menu_sid = xous::create_server().unwrap();
    let tour_menu_mgr = tourmenu::create_submenu(conn, actions_conn, tour_menu_sid);
    let gene_menu_sid = xous::create_server().unwrap();
    let gene_menu_mgr = genemenu::create_submenu(conn, actions_conn, gene_menu_sid);
    let idle_menu_sid = xous::create_server().unwrap();
    let idle_menu_mgr = idlemenu::create_submenu(conn, actions_conn, idle_menu_sid);

    let modals = modals::Modals::new(&xns).unwrap();

    // give the system a second to stabilize, then try to mount
    tt.sleep_ms(1000).ok();
    let pddb = pddb::Pddb::new();
    pddb.try_mount();
    vault_ui.apply_glyph_style();

    #[cfg(feature = "factory-new")]
    tests::reset_lifecycle();

    // this must init after PDDB is mounted
    let (global_config, init_mode) = GlobalConfig::init();
    let global_config = Arc::new(Mutex::new(global_config));
    *mode.lock().unwrap() = init_mode;
    vault_ui.set_global_config(global_config.clone());

    // overrides for testing
    #[cfg(feature = "production")]
    if is_developer {
        *mode.lock().unwrap() = VaultMode::Idle;
    }

    log::info!("initial mode: {:?}", *mode.lock().unwrap());

    // reload the database
    xous::send_message(
        actions_conn,
        xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .ok();
    vault_ui.refresh_draw_list();

    // kickstart the pumper
    xous::send_message(pump_conn, xous::Message::new_scalar(0, 0, 0, 0, 0))
        .expect("couldn't start the pumper");
    let mut menu_active = false;
    let mut jig_ready_seen = false;
    let mut boot_sent = false;
    loop {
        global_config.lock().unwrap().update_power_state(mode.lock().unwrap().clone());
        let msg = xous::receive_message(sid).unwrap();
        log::trace!("Got message: {:?}", msg.body.id());
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::Redraw) => {
                if !boot_sent {
                    // un-gate the power manager on our first "normal" UI loop iteration
                    boot_sent = true;
                    global_config.lock().unwrap().power_manager_boot_finished();
                }
                vault_ui.redraw();
            }
            Some(VaultOp::ReloadDbAndFullRedraw) => {
                xous::send_message(
                    actions_conn,
                    xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                vault_ui.refresh_draw_list();
                vault_ui.redraw();
            }
            Some(VaultOp::MenuDone) => {
                menu_active = false;
                // update the TOTP codes, in case there were changes
                vault_ui.refresh_draw_list();
                animate.store(true, Ordering::SeqCst);
                vault_ui.redraw();
            }
            Some(VaultOp::KeyPress) => xous::msg_scalar_unpack!(msg, k1, _k2, _k3, _k4, {
                let mode_now = *mode.lock().unwrap();
                let k = char::from_u32(k1 as u32).unwrap_or('\u{0000}');
                log::debug!("key {:x}", k1);

                // on the very first `~` received, this will transition a factory test state. In normal
                // operation this has no effect on the UI. But in factory test state this is an easy way to
                // monkey-patch the interlock on factory test to ensure that critical operations have
                // finished before proceeding.
                if !jig_ready_seen && k == '~' {
                    vault_ui.jig_ready();
                    jig_ready_seen = true;
                }
                if menu_active {
                    if matches!(mode_now, VaultMode::Tour) {
                        tour_menu_mgr.key_press(k);
                    } else if matches!(mode_now, VaultMode::ConfirmGene) {
                        gene_menu_mgr.key_press(k);
                    } else if matches!(mode_now, VaultMode::Idle)
                        || matches!(mode_now, VaultMode::IdleDevMode)
                    {
                        idle_menu_mgr.key_press(k);
                    } else {
                        menu_mgr.key_press(k);
                    }
                } else {
                    // let the UI get first whack at filtering keys - the '∴' key may be intercepted
                    // by various test routines
                    let k = vault_ui.handle_key(k);

                    match k.unwrap_or('\0') {
                        '∴' => {
                            animate.store(false, Ordering::SeqCst);
                            if matches!(mode_now, VaultMode::Tour) {
                                tour_menu_mgr.redraw();
                            } else if matches!(mode_now, VaultMode::ConfirmGene) {
                                gene_menu_mgr.redraw();
                            } else if matches!(mode_now, VaultMode::Idle)
                                || matches!(mode_now, VaultMode::IdleDevMode)
                            {
                                idle_menu_mgr.redraw();
                            } else {
                                menu_mgr.redraw();
                            }
                            menu_active = true;
                        }
                        '🔥' => {
                            vault_ui.camera_transition();
                            let prior_mode = animate.swap(false, Ordering::SeqCst);
                            xous::send_message(
                                actions_conn,
                                xous::Message::new_blocking_scalar(
                                    ActionOp::AcquireQr.to_usize().unwrap(),
                                    0,
                                    0,
                                    0,
                                    0,
                                ),
                            )
                            .ok();
                            // wait a moment for the last frame to clear before redrawing the UI
                            tt.sleep_ms(100).ok();
                            animate.store(prior_mode, Ordering::SeqCst);

                            if mode_now == VaultMode::Totp || mode_now == VaultMode::Password {
                                // reload DB to pickup the new data
                                xous::send_message(
                                    actions_conn,
                                    xous::Message::new_blocking_scalar(
                                        ActionOp::ReloadDb.to_usize().unwrap(),
                                        0,
                                        0,
                                        0,
                                        0,
                                    ),
                                )
                                .ok();
                                vault_ui.refresh_draw_list();
                            }
                            vault_ui.redraw();
                        }
                        '🔽' | '🔼' => {
                            log::info!("screen orientation event");
                        }
                        '⏰' => {
                            log::debug!("RTC wakeup event");
                        }
                        _ => {
                            log::trace!("debug: unhandled key to main {:?}", k);
                        }
                    }
                }
            }),
            Some(VaultOp::MenuEditStage1) => {
                // stage 1 happens here because the filtered list and selection entry are in the responsive UX
                // section.
                log::debug!("selecting entry for edit");
                // this will block redraws
                animate.store(false, Ordering::SeqCst);
                if let Some(entry) = vault_ui.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.lend(actions_conn, ActionOp::MenuEditStage2.to_u32().unwrap())
                        .expect("messaging error");
                } else {
                    modals.show_notification(t!("vault.error.nothing_selected", locales::LANG), None).ok();
                }
                animate.store(true, Ordering::SeqCst);
            }
            Some(VaultOp::MenuChangeFont) => {
                for item in FONT_LIST {
                    modals.add_list_item(item).expect("couldn't build radio item list");
                }
                animate.store(false, Ordering::SeqCst);
                match modals.get_radiobutton(t!("vault.select_font", locales::LANG)) {
                    Ok(style) => {
                        vault_ui.store_glyph_style(name_to_style(&style).unwrap_or(DEFAULT_FONT));
                        vault_ui.apply_glyph_style();
                    }
                    _ => log::error!("get_radiobutton failed"),
                }
                animate.store(true, Ordering::SeqCst);
            }
            Some(VaultOp::MenuDeleteStage1) => {
                animate.store(false, Ordering::SeqCst);
                if let Some(entry) = vault_ui.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.lend(actions_conn, ActionOp::MenuDeleteStage2.to_u32().unwrap())
                        .expect("messaging error");
                } else {
                    modals.show_notification(t!("vault.error.nothing_selected", locales::LANG), None).ok();
                }
                xous::send_message(
                    actions_conn,
                    xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                animate.store(true, Ordering::SeqCst);
                vault_ui.refresh_draw_list();
                vault_ui.redraw();
            }
            Some(VaultOp::MenuUsernames) => {
                // TODO: clean this up/consolidate with add_password() routine in actions.rs
                use std::path::Path;
                use std::{fs::File, io, io::BufRead, io::Write};
                animate.store(false, Ordering::SeqCst);
                let mut usernames: Vec<String> = if let Ok(file) = File::open(VAULT_CONFIG_USERNAMES) {
                    let reader = std::io::BufReader::new(file);
                    reader
                        .lines()
                        .collect::<io::Result<Vec<String>>>()
                        .map_err(|_| "Can't read usernames")
                        .unwrap()
                } else {
                    // Create new file and write default content
                    std::fs::create_dir_all(Path::new(VAULT_CONFIG_USERNAMES).parent().unwrap()).unwrap();
                    let mut file = File::create(VAULT_CONFIG_USERNAMES).unwrap();
                    file.write_all("".as_bytes()).unwrap();
                    Vec::new()
                };
                let mut usernames_refs: Vec<&str> = usernames.iter().map(AsRef::as_ref).collect();
                usernames_refs.push(t!("vault.add_new", locales::LANG));
                modals.add_list(usernames_refs).unwrap();
                match modals.get_radiobutton(t!("vault.username", locales::LANG)) {
                    Ok(response) => {
                        let placeholder = if response == t!("vault.add_new", locales::LANG) {
                            None
                        } else {
                            // remove the old username
                            usernames.retain(|u| *u != response);
                            Some(response)
                        };
                        let new_username = &modals
                            .alert_builder("")
                            .field(placeholder, None)
                            .build()
                            .map_err(|e| format!("Error entering username: {:?}", e))
                            .unwrap()
                            .content()[0]
                            .content;
                        usernames.push(new_username.to_owned());
                        let mut file = File::create(VAULT_CONFIG_USERNAMES)
                            .map_err(|e| format!("Couldn't open usernames file for writing: {:?}", e))
                            .unwrap();
                        for username in usernames {
                            writeln!(file, "{}", username)
                                .map_err(|e| format!("Couldn't write username: {:?}", e))
                                .unwrap();
                        }
                    }
                    Err(e) => log::error!("Error selecting user: {:?}", e),
                }
                xous::send_message(
                    actions_conn,
                    xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                animate.store(true, Ordering::SeqCst);
                vault_ui.refresh_draw_list();
                vault_ui.redraw();
            }
            Some(VaultOp::MenuFilter) => {
                let new_filter = &modals
                    .alert_builder("Filter by:")
                    .field(Some(vault_ui.get_filter()), None)
                    .build()
                    .map_err(|e| format!("Error getting filter: {:?}", e))
                    .unwrap()
                    .content()[0]
                    .content;
                vault_ui.filter(&new_filter);
                vault_ui.refresh_draw_list();
                vault_ui.redraw();
            }
            Some(VaultOp::BasisChange) => {
                vault_ui.basis_change();
                xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        VaultOp::ReloadDbAndFullRedraw.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
            }
            Some(VaultOp::ShowQr) => {
                let previous = animate.load(Ordering::SeqCst);
                animate.store(false, Ordering::SeqCst);
                let mut test_data = [0u8; 40];
                #[cfg(feature = "hosted-baosec")]
                let mut trng = bao1x_emu::trng::Trng::new(&xns).unwrap();
                #[cfg(not(feature = "hosted-baosec"))]
                let mut trng = bao1x_hal_service::trng::Trng::new(&xns).unwrap();
                trng.fill_bytes_via_next(&mut test_data);
                let encoded = base45::encode(&test_data);
                modals.show_notification("", Some(&encoded)).ok();
                animate.store(previous, Ordering::SeqCst);
            }
            Some(VaultOp::AbortQr) => {
                if global_config.lock().unwrap().is_badge_attached() {
                    animate.store(false, Ordering::SeqCst);
                    *mode.lock().unwrap() = VaultMode::Idle;
                    vault_ui.redraw();
                } else {
                    vault_ui.redraw();
                }
            }
            Some(VaultOp::HandleQr) => {
                let mode_now = { *mode.lock().unwrap() };

                // this routine mainly exists to repatriate QR data from the ActionManager into the
                // top-level context. This avoids us having to share every object into the ActionManager.
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s: IpcString = buffer.to_original::<IpcString, _>().unwrap();
                log::info!("mode: {:?}, s: {}", mode_now, s.s);

                match mode_now {
                    VaultMode::GeneScan
                    | VaultMode::ResponseGene { quantum: _ }
                    | VaultMode::ShowKey { quantum: _ } => {
                        match base45::decode(&s.s.as_bytes()) {
                            Ok(data) => {
                                log::debug!("b45dec: {:x?}", data);
                                if data[..DC34_HEADER.len()] == DC34_HEADER {
                                    // assume we're scanning their key
                                    if data.len() < DC34_HEADER.len() + size_of::<Nonce>() {
                                        log::error!("protocol error: key is not long enough");
                                        *mode.lock().unwrap() = VaultMode::Idle;
                                        animate.store(false, Ordering::SeqCst);
                                        continue;
                                    }
                                    let their_nonce = Nonce::from_slice(
                                        &data[DC34_HEADER.len()..DC34_HEADER.len() + size_of::<Nonce>()],
                                    );
                                    // also generate a new nonce for myself now
                                    global_config.lock().unwrap().generate_my_nonce();
                                    let gene = global_config.lock().unwrap().get_padded_gamete().unwrap();
                                    let aead = global_config.lock().unwrap().cipher();
                                    let payload = Payload { msg: &gene, aad: &[] };
                                    // encrypt returns ciphertext || nonce
                                    let ct_nonce = aead.encrypt(their_nonce, payload).unwrap();

                                    let mut response = Vec::new();
                                    response.extend_from_slice(&ct_nonce);

                                    log::debug!("raw {} bytes", response.len());
                                    let encoded = base45::encode(&response);
                                    log::debug!("encoding {} bytes", encoded.as_bytes().len());
                                    let code = QrCode::with_error_correction_level(
                                        encoded.as_bytes(),
                                        qrcode::EcLevel::M,
                                    )
                                    .expect("couldn't build QR code");
                                    log::info!(
                                        "Gene encoded {} bytes to Qrcode version {:?}",
                                        encoded.as_bytes().len(),
                                        code.version()
                                    );
                                    vault_ui.qr_override = Some(code);
                                    {
                                        *mode.lock().unwrap() = VaultMode::ResponseGene { quantum: 0 };
                                    }
                                    animate.store(true, Ordering::SeqCst);
                                } else {
                                    log::info!("Attempting gene decryption...");
                                    // try to decrypt a gene - could be either round 1 or round 2 of gene
                                    // decryption
                                    if data.len() < Aes256::block_size() + size_of::<Tag>() {
                                        log::error!("Protocol error: gene data too short");
                                        *mode.lock().unwrap() = VaultMode::Idle;
                                        animate.store(false, Ordering::SeqCst);
                                        modals.show_notification("Gene truncated", None).ok();
                                        continue;
                                    }
                                    let nonce1 = if let Some(nonce1) =
                                        global_config.lock().unwrap().get_my_nonce()
                                    {
                                        nonce1.clone()
                                    } else {
                                        *mode.lock().unwrap() = VaultMode::Idle;
                                        animate.store(false, Ordering::SeqCst);
                                        modals.show_notification("Mate must scan your key first!", None).ok();
                                        log::error!("nonce1 missing");
                                        continue;
                                    };
                                    log::debug!("nonce1: {:x?}", nonce1);
                                    // extract & save their nonce
                                    let aead = global_config.lock().unwrap().cipher();
                                    let payload = Payload { msg: &data, aad: &[] };
                                    log::debug!("payload: {:x?} {:x?}", payload.msg, payload.aad);
                                    match aead.decrypt(&nonce1, payload) {
                                        Ok(msg) => {
                                            log::debug!("decrypted {:x?}", msg);
                                            if let Some(mut sperm) =
                                                Haploid::deserialize(&msg[..size_of::<Haploid>()])
                                            {
                                                let incoming_type =
                                                    BadgeType::try_from(msg[15]).unwrap_or(BadgeType::None);

                                                // if reproducing among the same badge type, elevate
                                                // the mutation rate - adds more diversity more
                                                // quickly for populations that are isolated
                                                if incoming_type == global_config.lock().unwrap().badge_type()
                                                {
                                                    log::info!(
                                                        "Inbreeding detected, elevating mutation rate"
                                                    );
                                                    mutate(&mut sperm, MutationRate::Elevated);
                                                }

                                                // perform syngamy
                                                let egg = global_config.lock().unwrap().get_egg().unwrap();
                                                log::info!(
                                                    "Replacing individual with {:x?}, {:x?}",
                                                    egg,
                                                    sperm
                                                );
                                                global_config.lock().unwrap().replace_gene(egg, sperm);
                                                global_config.lock().unwrap().render_gene();

                                                // raise the confirmation menu
                                                {
                                                    *mode.lock().unwrap() = VaultMode::ConfirmGene;
                                                }
                                                // raise the menu for confirmation
                                                std::thread::spawn(move || {
                                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                                    xous::send_message(
                                                        conn,
                                                        xous::Message::new_scalar(
                                                            VaultOp::KeyPress.to_usize().unwrap(),
                                                            '∴' as u32 as usize,
                                                            0,
                                                            0,
                                                            0,
                                                        ),
                                                    )
                                                    .ok();
                                                });
                                                /*
                                                animate.store(false, Ordering::SeqCst);
                                                menu_active = true;

                                                gene_menu_mgr.redraw();
                                                */
                                            } else {
                                                log::error!("Failed to deserialize gene");
                                                *mode.lock().unwrap() = VaultMode::Idle;
                                                animate.store(false, Ordering::SeqCst);
                                                modals
                                                    .show_notification("Gene failed to deserialize", None)
                                                    .ok();
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("Failed to decrypt gene: {:?}", e);
                                            *mode.lock().unwrap() = VaultMode::Idle;
                                            animate.store(false, Ordering::SeqCst);
                                            modals.show_notification("Authentication error!", None).ok();
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("Invalid gene code: {:?} / {}", e, &s.s);
                                animate.store(false, Ordering::SeqCst);
                                modals.show_notification("Invalid gene code", None).ok();
                                *mode.lock().unwrap() = VaultMode::Idle;
                            }
                        }
                    }
                    _ => {
                        if let Some((request, data)) = s.s.split_once("://") {
                            match request {
                                "test" => {
                                    vault_ui.test_string(&s.s);
                                }
                                "factory" => {
                                    if data == crate::ux::FACTORY_STANDALONE_STRING {
                                        *mode.lock().unwrap() = VaultMode::StandAloneTest;
                                    }
                                }
                                _ => {
                                    log::warn!("Unhandled string in main: {}", &s.s);
                                    let mut qr_str = String::from(t!("vault.error.qr", locales::LANG));
                                    qr_str.push_str(&format!(": {}", &qr_str));
                                    modals.show_notification(&qr_str, None).ok();
                                }
                            }
                        }
                    }
                }
            }
            Some(VaultOp::TourContinue) => {
                // do nothing, the slide show will continue
            }
            Some(VaultOp::TourLater) => {
                if global_config.lock().unwrap().is_badge_attached() {
                    *mode.lock().unwrap() = VaultMode::Idle;
                } else {
                    *mode.lock().unwrap() = VaultMode::Password;
                    xous::send_message(
                        actions_conn,
                        xous::Message::new_blocking_scalar(
                            ActionOp::ReloadDb.to_usize().unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .ok();
                    vault_ui.refresh_draw_list();
                }
                log::info!("tour_later redraw");
                vault_ui.redraw();
            }
            Some(VaultOp::TourNever) => {
                *mode.lock().unwrap() = global_config.lock().unwrap().set_skip_tour(true);

                if *mode.lock().unwrap() == VaultMode::Password {
                    xous::send_message(
                        actions_conn,
                        xous::Message::new_blocking_scalar(
                            ActionOp::ReloadDb.to_usize().unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .ok();
                    vault_ui.refresh_draw_list();
                }
                vault_ui.redraw();
            }
            Some(VaultOp::KeepGene) => {
                if let Some(gene) = global_config.lock().unwrap().gene() {
                    save_light_gene(gene);
                }
                *mode.lock().unwrap() = VaultMode::Idle;
            }
            Some(VaultOp::RevertGene) => {
                global_config.lock().unwrap().revert_gene();
                global_config.lock().unwrap().render_gene();
                *mode.lock().unwrap() = VaultMode::Idle;
            }
            Some(VaultOp::TokenMode) => {
                *mode.lock().unwrap() = VaultMode::Password;
                xous::send_message(
                    actions_conn,
                    xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                vault_ui.refresh_draw_list();
                vault_ui.redraw();
            }
            Some(VaultOp::DefconHelp) => {
                *mode.lock().unwrap() = VaultMode::DefconHelp;
                vault_ui.reset_help_state();
                vault_ui.redraw();
            }
            Some(VaultOp::MenuTokenHelp) => {
                *mode.lock().unwrap() = VaultMode::TokenHelp;
                vault_ui.reset_token_help_state();
                vault_ui.redraw();
            }
            Some(VaultOp::BadgeMode) => {
                *mode.lock().unwrap() = VaultMode::Idle;
                vault_ui.redraw();
            }
            Some(VaultOp::About) => {
                *mode.lock().unwrap() = VaultMode::About;
                vault_ui.reset_about_state();
                vault_ui.redraw();
            }
            _ => {
                log::error!("Got unknown message: {:?}", msg);
            }
        }
    }
}

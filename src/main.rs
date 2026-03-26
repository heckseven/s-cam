mod ux;
use ux::*;
mod itemcache;
use itemcache::*;
mod actions;
use actions::ActionOp;
mod storage;
mod submenu;
mod totp;
mod tourmenu;
pub mod vault_api;
use ux_api::service::gfx::Gfx;
pub use vault_api::*;
mod bitmaps;
mod generator;
mod vendor_commands;

use core::sync::atomic::{AtomicBool, Ordering};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use dc34_vault::Transport;
use dc34_vault::ctap::main_hid::HidIterType;
use dc34_vault::env::Env;
use dc34_vault::env::xous::XousEnv;
use locales::t;
use num_traits::*;
use pddb::Pddb;
use totp::PumpOp;
use xous::msg_blocking_scalar_unpack;
use xous_ipc::Buffer;
use xous_usb_hid::device::fido::*;

use crate::vendor_commands::VendorSession;

/*
Dev status & notes --

UI interaction planning.

Main mode of interaction is QR code scanning. This should be accessible with a single button. Thus:

1. middle center button pops up QR code scanner. Behavior then depends on the code scanned.
  Note: will need a menu item to replace passwords - we should keep the old passwords in case it's needed?

Observation: left/right paging buttons don't do a lot with O(hundreds) passwords, but scrolling
is fast. So don't implement left/right paging as on Precursor, freeing up two buttons.

2. Left button: pops up text entry to filter lists

3. Right button: "action" button - used to type the current password, and/or approve FIDO sigs

4. Up/down/select jog: exclusively for menu interactions. Menus are always linear, with select.

This UI design does not allow for hierarchical menus because there isn't a "back" button, but
we *could*, possibly, if we really needed menu hierarchies, repurpose a left/right button as
a hierarchy nav function.

-> But can we keep the menu shallow?

Architectural notes --

Data is long-term stored in the PDDB. Each of the three modes have their own dictionary
(OpenSK/FIDO2, passwords, totp).

The data is read into an `ItemList`, which is a RAM-based structure that caches all the PDDB data for
fast sorting, searching etc. `ItemList` is where meta-operations like search & sort happen.

For rendering the data is then copied into a UI element, such as a `ScrollableList`, based on
the currently active mode.
  */

/*
  Note on factory test:
    - Use console tests (`test [foo]`) routines to check voltages, accelerometer ID
    - UI test is just for testing UI elements!

  DC34 interactions

  - Data / mode bits required:
    - Developer mode -> from keystore
    - Accel installed -> from power manager
    - Settings -> from PDDB keys 'dc34.screen', 'dc34.powoff'
    - Lightgene -> stored in 'dc34.lightgene', decrypted by key store
    - Badge type -> stored in 'dc34.type', plaintext
    - DC34 Ko -> stored in 'dc34.ko', decrypted by key store
    - PIN code is the basis encryption key -> from existing PDDB api
    - Show tour <bool> -> 'dc34.tour'
    # - Factory test <bool> -> 'dc34.factory' <- replaced by presence of DC34 Ko key
    - PIN type -> Enum that stores {None, Numeric, Qr} as options. this changes the API call to keystore for unwrapping system_keys.data

  - Lifecycle elements:
    - Ko provisioning - done by test jig in factory
      - "test setko <base64>": run after PDDB is init
      - base64 blob contains k0 plus hash
    - Badge type - set by pull-downs on SAO. 1/1/1 = not mounted
      - Memorized first time pull down encountered.
      - Light pattern regenerated at this point

  - If developer mode:
    - Flash defcon logo between two inverse options, fade in and out
    - Overlay 'dev mode' text
    - No lightgene functions available - any mode press goes to vault mode options, as if no accel available

  - Factory test:
    - "press in on jog dial"
    - "up/down/select"
    - "left/right"
    - "middle" -> qr scan

  - First time "cold on":
    - show tour
      - "Welcome to your DC34 Badge! / [Press any button to continue]"
      - "Push the jog wheel in to raise menus / Push again to select items"
      - Raise Menu:
         - continue -> next stage of tour
         - skip tour -> operating mode
         - never show again -> store never show again -> operating mode
      - "Your badge's light pattern is unique, encoded in a 'light gene'!"
      - "You can 'breed' your light gene, evolving new patterns. Here's how:"
      - "First, share your KEY by pressing either of the left or right buttons."
      - "A mate shows consent by scanning your KEY using their middle button."
      - "Finally, scan your mate's QR code using the middle button on your badge."
      - "Your mate can scan your new QR code if they also want your light genes."
      - "Badge Recap: (show KEY, down arrows) / (scan code, down arrow)"
      - "Your badge is also a FIDO token and password manager!"
      - "Detach the core module by removing two screws on the backside."
      - "Then, connect to a USB host to use your security token."
         -- start of "Help" sequence in token mode
      - "The right button toggles between TOTP and password mode."
      - "The middle button scans QR codes to enroll TOTPs."
      - "The left button 'auto-types' credentials into the USB host."
      - "Token Recap: (type) (scan) (mode)"
      - "You'll need a browser extension to set time and manage passwords."
      - "Go to baochip.com/defcon34 for more information. Enjoy the conference!"

  - If base board is detected:
    - enable DC34 Idle screen
    - return to Idle screen after INACTIVITY time

  - If base board not detected:
    - go to token mode immediately

  - DC34 Idle screen: black background logo, fading in and out.
    - Left/right buttons show up 'KEY'. KEY QR code is shown. Toggle "KEY" text on and off.
    - Middle button starts scanning. Camera preview comes up. Any button aborts.
    - After scanning, show 'GENE' qr code. Toggle 'GENE" text on and off in this mode. Any button closes.
    - Menu options:
       - Security token -> goes to security token mode
       - About -> goes to about sequence
       - Tour -> goes to tour sequence
       - Settings -> goes to settings menu
       - Close menu
   - About sequence:
     - Tech by bunnie [bunniestudios logo]
     - Powered by Baochip [Baochip logo]
     - Art by Cheeso [cheeso logo]
   - Settings - as dynamic menu:
     - Screen off: [] secs -> slider entry?
     - Power save: [] secs -> slider entry?
     - Close
   - Token mode:
     - Left button autotypes
     - Right button toggles between PW -> TOTP when detached. When attached PW -> TOTP -> Idle loop.
     - Middle button QR scans
     - Menu has Edit / Delete / Usernames / About / Help / Close
       - [optional - low priority] Filter -> if any entries, add filter string entry
       - Edit edits the current entry, if any
       - Delete deletes the current entry, if any
       - Usernames brings up list of usernames. If empty, prompt to enter new username.
       - [optional - medium priority] PIN code -> activates PIN menu
       - [optional - lowest priority] Backups
       - Help shows "Help" sequence in token mode
       - About shows about sequence

   PIN codes will require implementing the following:
     - In the keystore, a PIN factor needs to be added to the key derivation
     - This is a code supplied by the caller that is hashed into the master key, then used to unlock keys
     - The system encryption basis is [systemkeys.data] - this is encrypted on init to a no-PIN situation
     - if the PIN flag is set, then the PIN API has to be used to decrypt systemkeys.data
     - every time the PIN is changed, systemkeys.data ciphertext has to be re-stored into the SCD structure, based on
       the current wrapping of the systemkeys.data plaintext through the PIN configuration
   - PIN menu - if no PIN set:
     - Manual entry -> numeric entry
     - Generate QR -> "Save this QR code now! You won't be able to login without it." / "Select PIN code->Remove QR PIN to undo QR code login" -> show code -> then close
   - PIN menu - if manual PIN set:
     - Go directly to an edit screen of the existing PIN
   - PIN menu - if QR set:
     - Show QR code
     - Generate new QR code -> back to Generate QR sequence
     - Remove QR code PIN
*/

pub(crate) const SERVER_NAME_VAULT2: &str = "_Vault2_";
const DC34_DICT: &str = "dc34";
const DC34_SECRET: &str = "k0";
const DC34_TOUR: &str = "tour";
const DC34_TOKEN_TOUR: &str = "tokentour";
const DC34_BADGE: &str = "badge";

#[derive(Copy, Clone, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VaultMode {
    Idle, // has two variants, one for regular, other for developer mode
    FactoryTest,
    Tour,
    TokenTour,
    About,
    Totp,
    Password,
}

pub enum LifeCycle {
    // Exit condition: Ko provisioned
    BoardTest,
    // Exit condition: badge type assigned
    AssemblyTest,
    // Exit condition: dc34.tour is false
    FirstTime,
    Main,
}

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum BadgeType {
    Human = 3,
    Goon = 5,
    Village = 6,
    CtfContest = 1,
    Uber = 0,
    Community = 2,
    Other = 4,
    None = 7,
}
impl TryFrom<u8> for BadgeType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            3 => Ok(BadgeType::Human),
            5 => Ok(BadgeType::Goon),
            6 => Ok(BadgeType::Village),
            1 => Ok(BadgeType::CtfContest),
            0 => Ok(BadgeType::Uber),
            2 => Ok(BadgeType::Community),
            4 => Ok(BadgeType::Other),
            7 => Ok(BadgeType::None),
            n => Err(n),
        }
    }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct SelectedEntry {
    pub key_guid: String,
    pub description: String,
    pub mode: VaultMode,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Vault2 PID is {}", xous::process::id());

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
    let allow_totp_rendering = Arc::new(AtomicBool::new(true));
    let item_lists = Arc::new(Mutex::new(ItemLists::new()));
    let action_active = Arc::new(AtomicBool::new(false));
    // Protects access to the openSK PDDB entries from simultaneous readout on the UX while OpenSK is updating
    let opensk_mutex = Arc::new(Mutex::new(0));
    let allow_host = Arc::new(AtomicBool::new(false));

    // spawn the TOTP pumper
    let pump_sid = xous::create_server().unwrap();
    crate::totp::pumper(mode.clone(), pump_sid, conn, allow_totp_rendering.clone());
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
        conn,
        item_lists.clone(),
        mode.clone(),
        allow_totp_rendering.clone(),
        pump_conn,
        actions_conn,
    );

    let _ = thread::spawn({
        let main_conn = conn.clone();
        let sid = actions_sid.clone();
        let mode = mode.clone();
        let item_lists = item_lists.clone();
        let action_active = action_active.clone();
        move || {
            let mut manager = crate::actions::ActionManager::new(main_conn, mode, item_lists, action_active);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<ActionOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(ActionOp::MenuAddnew) => {
                        manager.activate();
                        manager.menu_addnew(); // this is responsible for updating the item cache
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuDeleteStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_delete(entry);
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuEditStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_edit(&entry); // this is responsible for updating the item cache
                        manager.update_db_entry(&entry);
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuUnlockBasis) => {
                        manager.activate();
                        manager.unlock_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords (totp/fido are not cached and don't need clearing)
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuManageBasis) => {
                        manager.activate();
                        manager.manage_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuClose) => {
                        // dummy activate/de-activate cycle because we have to trigger a redraw of the
                        // underlying UX
                        manager.activate();
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateOneItem) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.update_db_entry(&entry);
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateMode) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        // the password DBs are now not shared between modes, so no need to re-retrieve it.
                        if manager.is_db_empty() {
                            manager.retrieve_db();
                        }
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::ReloadDb) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::AcquireQr) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        manager.acquire_qr();
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::Quit) => {
                        break;
                    }
                    None => {
                        log::error!("msg could not be decoded {:?}", msg);
                    }
                    #[cfg(feature = "vault-testing")]
                    Some(ActionOp::GenerateTests) => {
                        manager.populate_tests();
                        manager.retrieve_db();
                    }
                }
            }
            xous::destroy_server(sid).ok();
        }
    });

    // spawn the FIDO2 USB handler
    let _ = thread::spawn({
        let allow_host = allow_host.clone();
        let opensk_mutex = opensk_mutex.clone();
        let conn = conn.clone();
        move || {
            let mut vendor_session = VendorSession::default();
            // block until the PDDB is mounted
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();

            let env = XousEnv::new(conn);
            let mut ctap = dc34_vault::Ctap::new(env, Instant::now());
            loop {
                match ctap.env().main_hid_connection().u2f_wait_incoming() {
                    Ok(msg) => {
                        ctap.update_timeouts(Instant::now());
                        let mutex = opensk_mutex.lock().unwrap();
                        log::trace!("Received U2F packet");
                        let typed_reply =
                            ctap.process_hid_packet(&msg.packet, Transport::MainHid, Instant::now());
                        match typed_reply {
                            HidIterType::Ctap(reply) => {
                                for pkt_reply in reply {
                                    let mut reply = RawFidoReport::default();
                                    reply.packet.copy_from_slice(&pkt_reply);
                                    let status = ctap.env().main_hid_connection().u2f_send(reply);
                                    match status {
                                        Ok(()) => {
                                            log::trace!("Sent U2F packet");
                                        }
                                        Err(e) => {
                                            log::error!("Error sending U2F packet: {:?}", e);
                                        }
                                    }
                                }
                            }
                            HidIterType::Vendor(msg) => {
                                let reply = match vendor_commands::handle_vendor_data(
                                    msg.cmd as u8,
                                    msg.cid,
                                    msg.payload,
                                    &mut vendor_session,
                                ) {
                                    Ok(return_payload) => {
                                        // if None, this means we've finished parsing all that
                                        // was needed, and we handle/respond with real data

                                        match return_payload {
                                            Some(data) => data,
                                            None => {
                                                log::debug!("starting processing of vendor data...");
                                                let resp = vendor_commands::handle_vendor_command(
                                                    &mut vendor_session,
                                                    allow_host.load(Ordering::SeqCst),
                                                );
                                                log::debug!("finished processing of vendor data!");

                                                match vendor_session.is_backup() {
                                                    true => {
                                                        if vendor_session.has_backup_data() {
                                                            resp
                                                        } else {
                                                            vendor_session = VendorSession::default();
                                                            resp
                                                        }
                                                    }
                                                    false => {
                                                        vendor_session = VendorSession::default();
                                                        resp
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(session_error) => {
                                        // reset the session
                                        vendor_session = VendorSession::default();

                                        session_error.ctaphid_error(msg.cid)
                                    }
                                };
                                for pkt_reply in reply {
                                    let mut reply = RawFidoReport::default();
                                    reply.packet.copy_from_slice(&pkt_reply);
                                    let status = ctap.env().main_hid_connection().u2f_send(reply);
                                    match status {
                                        Ok(()) => {
                                            log::trace!("Sent U2F packet");
                                        }
                                        Err(e) => {
                                            log::error!("Error sending U2F packet: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                        drop(mutex);
                    }
                    Err(e) => match e {
                        _ => {
                            log::warn!("FIDO listener got an error: {:?}", e);
                        }
                    },
                }
            }
        }
    });

    let menu_sid = xous::create_server().unwrap();
    let menu_mgr = submenu::create_submenu(conn, actions_conn, menu_sid);
    let tour_menu_sid = xous::create_server().unwrap();
    let tour_menu_mgr = tourmenu::create_submenu(conn, actions_conn, tour_menu_sid);

    let modals = modals::Modals::new(&xns).unwrap();
    vault_ui.apply_glyph_style();

    let keystore = keystore::Keystore::new(&xns);
    let is_developer = keystore.is_developer().expect("couldn't query developer mode");

    // TODO: replace with a function that actually checks the attachment pins
    let badge_attached = true;
    // TODO: replace with a function that actually checks cold boot status
    let cold_boot = true;

    // give the system a second to stabilize, then try to mount
    tt.sleep_ms(1000).ok();
    let pddb = pddb::Pddb::new();
    pddb.try_mount();

    // initialize the system state from the PDDB
    let mut k0 = [0u8; 32];
    let k0_len = read_pddb(&pddb, DC34_SECRET, &mut k0);
    log::info!("k0_len {}, k0 {:x?}", k0_len, k0);

    let mut skip_tour_buf = [0u8; 1];
    read_pddb(&pddb, DC34_TOUR, &mut skip_tour_buf);
    let skip_tour = skip_tour_buf[0] != 0;
    log::info!("skip_tour {:?},  {:x?}", skip_tour, skip_tour_buf);

    let mut skip_token_tour_buf = [0u8; 1];
    read_pddb(&pddb, DC34_TOKEN_TOUR, &mut skip_token_tour_buf);
    let skip_token_tour = skip_token_tour_buf[0] != 0;
    log::info!("skip_token_tour {:?},  {:x?}", skip_token_tour, skip_token_tour_buf);

    let mut badge_code = [BadgeType::None as u8; 1];
    let badge_code_len = read_pddb(&pddb, DC34_BADGE, &mut badge_code);
    let badge_type = if badge_code_len == 0 {
        BadgeType::None
    } else {
        BadgeType::try_from(badge_code[0]).unwrap_or(BadgeType::None)
    };

    // set the initial mode based on the following state
    *mode.lock().unwrap() = if badge_type == BadgeType::None {
        VaultMode::FactoryTest
    } else if badge_attached {
        if skip_tour || !cold_boot { VaultMode::Idle } else { VaultMode::Tour }
    } else {
        if skip_token_tour { VaultMode::Password } else { VaultMode::TokenTour }
    };

    #[cfg(feature = "production")]
    if is_developer {
        *mode.lock().unwrap() = VaultMode::Idle;
    }
    *mode.lock().unwrap() = VaultMode::Tour;

    log::info!("initial mode: {:?}", *mode.lock().unwrap());

    // reload the database
    xous::send_message(
        actions_conn,
        xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .ok();
    vault_ui.refresh_draw_list();

    // kickstart the pumper
    xous::send_message(pump_conn, xous::Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0))
        .expect("couldn't start the pumper");
    let mut menu_active = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::trace!("Got message: {:?}", msg.body.id());
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::Redraw) => {
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
                allow_totp_rendering.store(true, Ordering::SeqCst);
                vault_ui.redraw();
            }
            Some(VaultOp::KeyPress) => xous::msg_scalar_unpack!(msg, k1, _k2, _k3, _k4, {
                let k = char::from_u32(k1 as u32).unwrap_or('\u{0000}');
                log::debug!("key {:x}", k1);
                if menu_active {
                    if *mode.lock().unwrap() == VaultMode::Tour {
                        tour_menu_mgr.key_press(k);
                    } else {
                        menu_mgr.key_press(k);
                    }
                } else {
                    // let the UI get first whack at filtering keys - the '∴' key may be intercepted
                    // by various test routines
                    let k = vault_ui.handle_key(k);

                    match k.unwrap_or('\0') {
                        '∴' => {
                            allow_totp_rendering.store(false, Ordering::SeqCst);
                            if *mode.lock().unwrap() == VaultMode::Tour {
                                tour_menu_mgr.redraw();
                            } else {
                                menu_mgr.redraw();
                            }
                            menu_active = true;
                        }
                        '🔥' => {
                            allow_totp_rendering.store(false, Ordering::SeqCst);
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
                            allow_totp_rendering.store(true, Ordering::SeqCst);
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
                            vault_ui.redraw();
                        }
                        '⏯' => {
                            log::info!("accel event");
                        }
                        _ => {
                            log::trace!("unhandled key {:?}", k);
                        }
                    }
                }
            }),
            Some(VaultOp::MenuEditStage1) => {
                // stage 1 happens here because the filtered list and selection entry are in the responsive UX
                // section.
                log::debug!("selecting entry for edit");
                // this will block redraws
                allow_totp_rendering.store(false, Ordering::SeqCst);
                if let Some(entry) = vault_ui.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.lend(actions_conn, ActionOp::MenuEditStage2.to_u32().unwrap())
                        .expect("messaging error");
                } else {
                    modals.show_notification(t!("vault.error.nothing_selected", locales::LANG), None).ok();
                }
                allow_totp_rendering.store(true, Ordering::SeqCst);
            }
            Some(VaultOp::MenuChangeFont) => {
                for item in FONT_LIST {
                    modals.add_list_item(item).expect("couldn't build radio item list");
                }
                allow_totp_rendering.store(false, Ordering::SeqCst);
                match modals.get_radiobutton(t!("vault.select_font", locales::LANG)) {
                    Ok(style) => {
                        vault_ui.store_glyph_style(name_to_style(&style).unwrap_or(DEFAULT_FONT));
                        vault_ui.apply_glyph_style();
                    }
                    _ => log::error!("get_radiobutton failed"),
                }
                allow_totp_rendering.store(true, Ordering::SeqCst);
            }
            Some(VaultOp::MenuDeleteStage1) => {
                allow_totp_rendering.store(false, Ordering::SeqCst);
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
                allow_totp_rendering.store(true, Ordering::SeqCst);
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
                let previous = allow_totp_rendering.load(Ordering::SeqCst);
                allow_totp_rendering.store(false, Ordering::SeqCst);
                let mut test_data = [0u8; 40];
                #[cfg(feature = "hosted-baosec")]
                let mut trng = bao1x_emu::trng::Trng::new(&xns).unwrap();
                #[cfg(not(feature = "hosted-baosec"))]
                let mut trng = bao1x_hal_service::trng::Trng::new(&xns).unwrap();
                trng.fill_bytes_via_next(&mut test_data);
                let encoded = base45::encode(&test_data);
                modals.show_notification("", Some(&encoded)).ok();
                allow_totp_rendering.store(previous, Ordering::SeqCst);
            }
            Some(VaultOp::HandleQr) => {
                // this routine mainly exists to repatriate QR data from the ActionManager into the
                // top-level context. This avoids us having to share every object into the ActionManager.
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s: IpcString = buffer.to_original::<IpcString, _>().unwrap();
                if let Some((request, _data)) = s.s.split_once("://") {
                    match request {
                        "test" => {
                            vault_ui.test_string(&s.s);
                        }
                        _ => log::warn!("Unhandled string in main: {}", &s.s),
                    }
                }
            }
            Some(VaultOp::TourContinue) => {
                // do nothing, the slide show will continue
            }
            Some(VaultOp::TourLater) => {
                if badge_attached {
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
                let mut key = pddb
                    .get(DC34_DICT, DC34_TOUR, None, true, true, Some(1), None::<fn()>)
                    .expect("couldn't get PDDB key");
                key.write(&[1]).ok();
                if badge_attached {
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
                vault_ui.redraw();
            }
            _ => {
                log::error!("Got unknown message: {:?}", msg);
            }
        }
    }
}

pub fn read_pddb(pddb: &Pddb, key: &str, buf: &mut [u8]) -> usize {
    let mut key = pddb
        .get(DC34_DICT, key, None, true, true, Some(buf.len()), None::<fn()>)
        .expect("couldn't get PDDB key");
    key.read(buf).expect("couldn't read key")
}

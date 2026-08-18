use core::fmt::Write as TextViewWrite;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use bao1x_hal_service::Adc;
use blitstr2::GlyphStyle;

/// Patterns implemented in dc34-console; index 0 restores gene expression.
const LED_PATTERN_COUNT: usize = 9;
use chrono::Datelike;
use qrcode::{Color, QrCode};
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;
use ux_api::widgets::ScrollableList;
use xous::CID;

use crate::action_handler::SelectedEntry;
use crate::actions::ActionOp;
use crate::config::AttachState;
use crate::*;

const FAST_SCROLL_DELAY_MS: u64 = 1300;
const KEYUP_DELAY_MS: u64 = 100;
/// How many elements to skip through on fast scroll
const PAGE_INCREMENT: usize = 6;

const FACTORY_QR_STRING: &'static str = "test://factory-test-data-lorem-ipsum-data-data";
const JIG_TIMEOUT: u64 = 35;
// full string on QR code needs to be factory://factory-aae949f6969-lorem-ipsum-data
pub const FACTORY_STANDALONE_STRING: &'static str = "factory-aae949f6969-lorem-ipsum-data";

#[cfg(not(feature = "uber"))]
const LOWBATT_THRESH_MV: u32 = 2380;
#[cfg(feature = "uber")]
// loading is fairly light (80 mA << 0.2C) so 3200 should be darn near empty
const LOWBATT_THRESH_MV: u32 = 3200;
// how long to stay in low batt mode before forcing a sleep
#[cfg(not(feature = "uber"))]
const LOWBATT_TIMEOUT_S: u64 = 90;
#[cfg(feature = "uber")]
const LOWBATT_TIMEOUT_S: u64 = 180;

pub const DEFAULT_FONT: GlyphStyle = crate::theme::FONT;
pub fn name_to_style(name: &str) -> Option<GlyphStyle> {
    match name {
        "regular" => Some(GlyphStyle::Regular),
        "tall" => Some(GlyphStyle::Tall),
        "mono" => Some(GlyphStyle::Monospace),
        "cjk" => Some(GlyphStyle::Cjk),
        "bold" => Some(GlyphStyle::Bold),
        "large" => Some(GlyphStyle::Large),
        "small" => Some(GlyphStyle::Small),
        _ => None,
    }
}
const VAULT_CONFIG_DICT: &'static str = "vault.config";
const VAULT_CONFIG_KEY_FONT: &'static str = "fontstyle";

#[derive(PartialEq, Eq, Clone, Copy)]
enum DisplayOrientation {
    Normal,
    UpsideDown,
}

enum FactoryTestState {
    InitWait { start_time: Instant },
    JogPress { seen_press: bool },
    Up { seen_up: bool },
    Down { seen_down: bool },
    Left { seen_left: bool },
    Right { seen_right: bool },
    MiddleScan { seen_middle: bool, got_scan: bool },
    Finish,
    Error(String),
}

impl FactoryTestState {
    fn handle_input(
        self,
        k: Option<char>,
        qr_result: Option<String>,
        err: Option<String>,
        jig_ready: bool,
    ) -> Self {
        if let Some(e) = err {
            Self::Error(e)
        } else {
            match self {
                Self::InitWait { start_time } => {
                    // the test will start on its own after a few seconds regardless of jig input
                    if jig_ready
                        || std::time::Instant::now().duration_since(start_time).as_secs() > JIG_TIMEOUT
                    {
                        if jig_ready {
                            log::info!("start reason: received message");
                        } else {
                            log::info!("start reason: timeout");
                        }
                        log::info!("_|TT|_JIG.START,_|TE|_");
                        Self::JogPress { seen_press: false }
                    } else {
                        Self::InitWait { start_time }
                    }
                }
                Self::JogPress { seen_press } => {
                    let seen_press = seen_press || k.unwrap_or('\0') == '∴';
                    if seen_press {
                        log::info!("_|TT|_INTERACT.START,_|TE|_");
                        Self::Up { seen_up: false }
                    } else {
                        Self::JogPress { seen_press }
                    }
                }

                Self::Up { seen_up } => {
                    let seen_up = seen_up || k.unwrap_or('\0') == '↑';

                    if seen_up { Self::Down { seen_down: false } } else { Self::Up { seen_up } }
                }

                Self::Down { seen_down } => {
                    let seen_down = seen_down || k.unwrap_or('\0') == '↓';

                    if seen_down { Self::Left { seen_left: false } } else { Self::Down { seen_down } }
                }

                Self::Left { seen_left } => {
                    // note: left/right is swapped because PCB is upside-down during testing
                    let seen_left = seen_left || k.unwrap_or('\0') == '→';

                    if seen_left { Self::Right { seen_right: false } } else { Self::Left { seen_left } }
                }

                Self::Right { seen_right } => {
                    // note: left/right is swapped because PCB is upside-down during testing
                    let seen_right = seen_right || k.unwrap_or('\0') == '←';

                    if seen_right {
                        Self::MiddleScan { seen_middle: false, got_scan: false }
                    } else {
                        Self::Right { seen_right }
                    }
                }

                Self::MiddleScan { seen_middle, got_scan } => {
                    let seen_middle = seen_middle || k.unwrap_or('\0') == '🔥';
                    let got_scan =
                        got_scan || if let Some(qr) = qr_result { &qr == FACTORY_QR_STRING } else { false };

                    if seen_middle && got_scan {
                        Self::Finish
                    } else {
                        Self::MiddleScan { seen_middle, got_scan }
                    }
                }

                other => other,
            }
        }
    }
}

// Five state machines used to sit here: StandAloneTestState, TourState, HelpState,
// TokenHelpState, TokenTourState and an AboutState slideshow, driven by a tour_advance!
// macro. Every one was write-only - constructed in VaultUi::new, reset by opcodes no menu
// sends, and never once read by a redraw or a key handler. They were the DEFCON badge's
// guided tour, which S-CAM does not have. FactoryTestState above is the one that is still
// driven, from update_factory_test().
/// Centralizes tunable UI parameters for TOTP
struct TotpLayout {}
impl TotpLayout {
    /// Everything sits below the heading strip, so this screen is laid out like the others.
    const TOP: isize = crate::theme::LABEL_BAR_H;

    pub fn totp_box() -> RoundedRectangle {
        RoundedRectangle::new(
            Rectangle::new(Point::new(0, Self::TOP), Point::new(127, Self::TOP + 34)),
            0,
        )
    }

    /// Vertical margin for the font because the centering algorithm also aligns-top, and we want a little
    /// more verticale space for aesthetic reasons than the centering algorithm gives by default.
    pub fn totp_font_vmargin() -> Point { Point::new(0, 4) }

    pub fn totp_margin() -> Point { Point::new(10, 0) }

    pub fn totp_font() -> GlyphStyle { GlyphStyle::ExtraLarge }

    pub fn timer_box() -> Rectangle {
        Rectangle::new(Point::new(0, Self::TOP + 34), Point::new(127, Self::TOP + 42))
    }

    /// The list stops short of the bottom to leave the button bar room. Running it to 127
    /// drew rows underneath the labels.
    pub fn list_box() -> Rectangle {
        Rectangle::new(Point::new(0, Self::TOP + 42), Point::new(127, 127 - crate::theme::LABEL_BAR_H))
    }

    pub fn list_font() -> GlyphStyle { crate::theme::FONT }
}

pub struct VaultUi {
    #[allow(dead_code)]
    main_cid: CID,
    actions_conn: CID,
    gfx: Gfx,
    display_list: ScrollableList,
    item_lists: Arc<Mutex<ItemLists>>,
    mode: Arc<Mutex<VaultMode>>,
    animate: Arc<AtomicBool>,
    global_config: Option<Arc<Mutex<GlobalConfig>>>,
    orientation: DisplayOrientation,
    filter: String,

    /// totp redraw state
    totp_code: Option<String>,
    last_epoch: u64,

    pddb: RefCell<Pddb>,
    item_height: isize,
    style: GlyphStyle,
    screen_size: Point,

    usb_dev: usb_bao1x::UsbHid,
    last_key_time: u64,
    start_hold_time: u64,
    tt: ticktimer_server::Ticktimer,

    // the one state machine still driven
    factory_test: FactoryTestState,

    // when Some(), override the display state with this String in QR code format
    pub qr_override: Option<QrCode>,
    /// full URL behind qr_override, captioned under the code and scrolled if too long
    pub qr_caption: Option<String>,
    /// index into the console's pattern set; 0 means gene expression
    led_pattern: usize,
    /// PASSKEYS: FIDO2 registrations, loaded on entering the screen
    passkey_cache: Vec<crate::storage::Passkey>,
    passkey_cursor: usize,
    /// PHOTOS: captured images, keyed in vault.photos
    photo_cache: Vec<String>,
    photo_cursor: usize,
    /// BLING: standby image choices
    bling_cursor: usize,
    /// which standby image is in force; index into BUILTIN_IMAGES then photos
    standby_choice: usize,
    /// a just-taken photo held for the preview screen, before the user keeps it
    pending_photo: Option<[u32; 512]>,
    /// Which stored photo `pending_photo` currently holds, when it came from storage rather
    /// than the camera. Without this, "is a photo loaded?" was the whole test, so selecting a
    /// different one and exporting it re-sent the previous photo's bits.
    photo_loaded_key: Option<String>,
    /// what the standby screen currently has painted, so it is only repainted on change
    standby_drawn: Option<(usize, bool)>,
    /// BLINKY: 0 is gene expression, 1..=LED_PATTERN_COUNT are standalone patterns
    blinky_cursor: usize,
    // URL to display in ShowUrl mode (always validated by SanitizedUrl::new)
    pub show_url: Option<String>,

    // Bookmark list cache: (pddb_key, display_text, label). Populated by load_bookmarks().
    bookmark_cache: Vec<(String, String, String)>,
    // Index of the currently highlighted bookmark in bookmark_cache
    bookmark_cursor: usize,
    // Zero on the frame after the cursor or screen changed, which forces a full repaint.
    list_quantum: u32,
    // When the focused row last changed. The marquee is timed from this rather than counted
    // in redraws, so "held for a second" means a second.
    list_focus_ms: u64,

    // adc for reading battery level
    adc: Adc,
    batt_polled: bool,
    low_batt_since: Option<Instant>,

    pub user_bitmap: Option<[u32; 512]>,
    edge: bool,
    last_mode: VaultMode,
    pub bio_loaded: bool,
}

/// Move a list cursor one step, wrapping at both ends.
///
/// Every list screen shares this so they behave the same way: on a 128px panel a list runs
/// off the bottom, and stopping dead at the first row makes the last entries feel out of
/// reach. An empty list has nowhere to go.
fn step_cursor(cursor: usize, len: usize, up: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if up {
        if cursor == 0 { len - 1 } else { cursor - 1 }
    } else if cursor + 1 >= len {
        0
    } else {
        cursor + 1
    }
}

/// Standby images that ship with the firmware. Captured photos are appended to this list
/// at runtime, so "set a photo as the standby image" needs no separate mechanism.
pub const BUILTIN_IMAGES: [&str; 2] = ["S-CAM", "DEFCON"];

/// Index into BUILTIN_IMAGES for the DEFCON logo. Named because the idle draw has to tell
/// it from the S-CAM logo, and both are "no user bitmap".
pub const DEFCON_IMAGE: usize = 1;

/// Blinky choices. Index 0 is gene expression - the badge's protected behaviour and the
/// default - and the rest map onto dc34-console's pattern table.
pub const BLINKY_CHOICES: [&str; 10] = [
    "GENE (DEFAULT)",
    "RAINBOW",
    "RAINBOW SPIN",
    "RAINBOW WAVE",
    "COMET",
    "CHASE",
    "BREATHE",
    "EMBER",
    "BIRD",
    "RIOT",
];

impl VaultUi {
    /// Apply a standby-image choice: 0 and 1 are the built-ins, higher indices are photos.
    ///
    /// A captured photo is already in the badge's bitmap format, so "use this photo as the
    /// standby image" is a straight copy into user_bitmap with no conversion.
    pub(crate) fn apply_standby_choice(&mut self, choice: usize) {
        match choice {
            // Both built-ins are drawn from flash, not from user_bitmap; the idle draw picks
            // between them on standby_choice.
            0 | DEFCON_IMAGE => self.user_bitmap = None,
            n => {
                let idx = n - BUILTIN_IMAGES.len();
                if let Some(key) = self.photo_cache.get(idx).cloned() {
                    match crate::storage::photo_get(&self.pddb.borrow(), &key) {
                        Some(bits) => self.user_bitmap = Some(bits),
                        None => log::warn!("standby image {} could not be read", key),
                    }
                }
            }
        }
        self.standby_choice = choice;
    }

    /// Refresh the PASSKEYS list from PDDB.
    pub(crate) fn load_passkeys(&mut self) {
        self.passkey_cache = crate::storage::passkey_list(&self.pddb.borrow());
        self.passkey_cursor = 0;
    }

    /// Refresh the PHOTOS list from PDDB.
    pub(crate) fn load_photos(&mut self) {
        self.photo_cache = crate::storage::photo_list(&self.pddb.borrow());
        self.photo_cursor = 0;
    }

    pub fn new(
        xns: &xous_names::XousNames,
        cid: xous::CID,
        item_lists: Arc<Mutex<ItemLists>>,
        mode: Arc<Mutex<VaultMode>>,
        animate: Arc<AtomicBool>,
        actions_conn: xous::CID,
    ) -> Self {
        let pddb = pddb::Pddb::new();
        let mut totp_list = ScrollableList::default();
        totp_list
            .set_margin(TotpLayout::totp_margin())
            .pane_size(TotpLayout::list_box())
            .style(TotpLayout::list_font());
        totp_list.set_autoflush(false);

        let tt = ticktimer_server::Ticktimer::new().unwrap();
        let now = tt.elapsed_ms();
        let gfx = Gfx::new(&xns).unwrap();
        let style = DEFAULT_FONT;
        let glyph_height = gfx.glyph_height_hint(style).unwrap() as isize;
        let screen_size = gfx.screen_size().unwrap();
        let height = screen_size.y;
        Self {
            main_cid: cid,
            actions_conn,
            gfx,
            screen_size,
            display_list: totp_list,
            item_lists,
            mode,
            animate,
            filter: String::new(),
            orientation: DisplayOrientation::Normal,
            totp_code: None,
            last_epoch: crate::totp::get_current_unix_time().expect("couldn't get current time") / 30,
            pddb: RefCell::new(pddb),
            item_height: height / glyph_height,
            style,
            usb_dev: usb_bao1x::UsbHid::new(),
            tt,
            last_key_time: now,
            start_hold_time: now,
            factory_test: FactoryTestState::InitWait { start_time: std::time::Instant::now() },
            global_config: None,
            qr_override: None,
            qr_caption: None,
            led_pattern: 0,
            passkey_cache: Vec::new(),
            passkey_cursor: 0,
            photo_cache: Vec::new(),
            photo_cursor: 0,
            bling_cursor: 0,
            standby_choice: 0,
            pending_photo: None,
            photo_loaded_key: None,
            standby_drawn: None,
            blinky_cursor: 0,
            show_url: None,
            bookmark_cache: Vec::new(),
            bookmark_cursor: 0,
            list_quantum: 0,
            list_focus_ms: 0,
            adc: Adc::new(),
            batt_polled: false,
            low_batt_since: None,
            user_bitmap: None,
            edge: false,
            last_mode: VaultMode::Idle,
            bio_loaded: false,
        }
    }

    /// Load bookmarks from PDDB into the local cache. Call this before entering BookmarkList mode.
    pub(crate) fn load_bookmarks(&mut self) {
        use std::io::Read as StdRead;
        self.bookmark_cache.clear();
        self.bookmark_cursor = 0;
        self.list_quantum = 0;
        self.list_focus_ms = self.tt.elapsed_ms();
        let pddb = self.pddb.borrow();
        let keys = match pddb.list_keys(VAULT_BOOKMARKS_DICT, None) {
            Ok(k) => k,
            Err(e) => {
                log::warn!("load_bookmarks: list_keys failed: {:?}", e);
                return;
            }
        };
        let mut entries: Vec<(String, String, String)> = Vec::new();
        for key in keys.iter().filter(|k| k.as_str() != VAULT_BOOKMARKS_COUNTER_KEY) {
            if let Ok(mut entry) = pddb.get(
                VAULT_BOOKMARKS_DICT,
                key,
                None,
                false,
                false,
                None,
                None::<fn()>,
            ) {
                let mut data = Vec::new();
                if entry.read_to_end(&mut data).is_ok() {
                    if let Ok(body) = std::str::from_utf8(&data) {
                        let mut parts = body.splitn(3, '\n');
                        let url = parts.next().unwrap_or("").to_string();
                        let label = parts.next().unwrap_or("").to_string();
                        // Keep the whole URL. The list marquees the focused row, so
                        // truncating here only threw away the tail - which on a URL is
                        // usually the part that tells two entries apart.
                        entries.push((key.clone(), url, label));
                    }
                }
            }
        }
        // Sort by key (zero-padded hex u64) so entries appear in insertion order
        entries.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
        self.bookmark_cache = entries;
    }



    pub fn reset_factory_test(&mut self) {
        self.factory_test = FactoryTestState::InitWait { start_time: std::time::Instant::now() };
    }

    pub fn set_global_config(&mut self, config: Arc<Mutex<GlobalConfig>>) {
        self.global_config = Some(config);
    }

    pub(crate) fn refresh_draw_list(&mut self) {
        let mode = { (*self.mode.lock().unwrap()).clone() };

        let mut locked_lists = if let Ok(g) = self.item_lists.try_lock() {
            g
        } else {
            log::warn!("Couldn't get lock in refresh_draw_list; aborting the refresh");
            return;
        };
        let full_list = locked_lists.filtered_list(mode);
        self.display_list.clear();
        for item in full_list {
            self.display_list.add_item(0, &item.name());
        }
    }

    pub(crate) fn update_selected_totp_code(&mut self) -> Option<String> {
        if *self.mode.lock().unwrap() != VaultMode::Totp {
            return None;
        }
        if self.display_list.len() > 0 {
            let selected = self.display_list.get_selected();
            let mut locked_lists = self.item_lists.lock().unwrap();
            let full_list = locked_lists.full_list(VaultMode::Totp);
            if let Some(selected_item) = full_list.iter().find(|item| item.name() == selected) {
                match crate::totp::db_str_to_code(&selected_item.extra) {
                    Ok(s) => {
                        self.totp_code = Some(s.clone());
                        Some(s)
                    }
                    _ => {
                        self.totp_code = None;
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(crate) fn get_selected_item(&self) -> Option<ListItem> {
        let mode = *self.mode.lock().unwrap();
        if self.display_list.len() > 0 {
            let selected = self.display_list.get_selected();
            let mut locked_lists = self.item_lists.lock().unwrap();
            let full_list = locked_lists.full_list(mode);
            full_list.iter().find(|&item| item.name() == selected).cloned()
        } else {
            None
        }
    }

    pub(crate) fn selected_entry(&self) -> Option<SelectedEntry> {
        let mode = *self.mode.lock().unwrap();
        if let Some(li) = self.get_selected_item() {
            let name = li.name().to_owned();
            Some(SelectedEntry { key_guid: li.guid, description: name, mode })
        } else {
            None
        }
    }

    pub(crate) fn basis_change(&mut self) {
        self.item_lists.lock().unwrap().clear_all();
        self.display_list.clear();
    }

    pub(crate) fn apply_glyph_style(&mut self) {
        let style = match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            true,
            true,
            Some(32),
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut style_key) => {
                let mut name_bytes = Vec::<u8>::new();
                match style_key.read_to_end(&mut name_bytes) {
                    Ok(_len) => {
                        log::debug!(
                            "name_bytes: {:?} {:?}",
                            name_bytes,
                            String::from_utf8(name_bytes.to_vec())
                        );
                        name_to_style(&String::from_utf8(name_bytes).unwrap_or("regular".to_string()))
                            .unwrap_or(GlyphStyle::Regular)
                    }
                    Err(_) => GlyphStyle::Regular,
                }
            }
            _ => {
                log::warn!("PDDB access error reading default glyph size");
                GlyphStyle::Regular
            }
        };
        self.display_list.style(style);
        let glyph_height = self.gfx.glyph_height_hint(style).unwrap();
        self.item_height = glyph_height as isize + 2; // +2 because of the border width
        self.item_lists
            .lock()
            .unwrap()
            .set_items_per_screen((self.screen_size.y - 2 * self.item_height) / self.item_height);
        self.style = style;
    }

    /// Clear the entire screen.
    pub fn clear_area(&self) { self.gfx.clear().ok(); }

    /// Show a notification: confirm that an action finished, then put the screen back.
    ///
    /// This is its own UI pattern, distinct from a modal. A modal asks a question and waits
    /// for an answer; a notification states a fact that is already true - the photo saved,
    /// the export finished - so there is nothing to answer. Making the user press a key to
    /// acknowledge it adds a step to every save without telling them anything they did not
    /// already know. It paints centred over the current screen, holds long enough to read,
    /// then redraws whatever was underneath: a menu, a list or an image.
    pub(crate) fn notify(&mut self, text: &str) {
        const HOLD_MS: usize = 1200;
        let msg = text.to_lowercase();

        // Size the band to the wrapped text before placing it: a TextView whose bounds are
        // too small aborts typesetting rather than clipping, which would show nothing at all.
        // Ask the server how wide a cell is rather than assuming 7px from the font tables.
        // It is 8, and assuming 7 here made this band think seventeen characters fit when
        // fifteen do - so "URL typed to host" was typeset as "url typed to". The same wrong
        // assumption cost the list rows and the button bar a fix each; this was the last
        // place still making it.
        let cell = ux_api::widgets::cell_width(&self.gfx, crate::theme::FONT);
        let cols = ((self.screen_size.x - 4) / cell).max(1) as usize;
        let lines = (msg.chars().count().max(1) + cols - 1) / cols;
        // One row of slack. Sizing the band at exactly item_height * lines leaves the
        // server no room for its own inset, and a TextView that overruns is dropped rather
        // than clipped - so the notification silently drew nothing at all. It only appeared
        // once the cell width was corrected and a second line was needed, which is why this
        // looked like the fix causing the bug.
        let band = self.item_height * (lines as isize + 1);

        self.clear_area();
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::CenteredTop(Rectangle::new(
                Point::new(0, (self.screen_size.y - band) / 2),
                Point::new(self.screen_size.x, (self.screen_size.y + band) / 2),
            )),
        );
        tv.style = crate::theme::FONT;
        tv.draw_border = false;
        tv.invert = true;
        tv.margin = Point::new(2, 0);
        write!(tv, "{}", msg).ok();
        self.gfx.draw_textview(&mut tv).ok();
        self.gfx.flush().ok();

        self.tt.sleep_ms(HOLD_MS).ok();
        // Force a full repaint of whatever is underneath. Two screens paint only what has
        // changed since last time - the standby image, and any list that is mid-marquee - and
        // a notification has just painted over the whole panel. Without resetting both, the
        // redraw below repaints one list row over the notification and leaves the rest of it
        // on screen, which reads as "it never went back to the list".
        self.standby_drawn = None;
        self.list_quantum = 0;
        self.redraw();
    }

    /// Redraw the text view onto the screen.
    pub fn redraw(&mut self) {
        // to reduce locking thrash, we cache a copy of the current mode at the top of redraw.
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();

        // this check is can run at the top of every loop because the underlying implementation
        // caches the setting and only sends a message to the manager thread if there's a state change.
        if mode_at_entry == VaultMode::Idle {
            self.global_config.as_mut().unwrap().lock().unwrap().display_fading(true);
        } else {
            self.global_config.as_mut().unwrap().lock().unwrap().display_fading(false);
        }
        log::debug!("redraw mode: {:?}", mode_at_entry);

        match mode_at_entry {
            VaultMode::Passkeys => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "PASSKEYS");
                let rows: Vec<String> =
                    self.passkey_cache.iter().map(|p| p.name.clone()).collect();
                crate::theme::list(
                    &self.gfx, self.screen_size, self.item_height,
                    &rows, self.passkey_cursor, "no passkeys stored",
                    crate::theme::ListStyle::Ghost,
                    None,
                    crate::theme::Repaint::All,
                );
                let has = !rows.is_empty();
                crate::theme::button_labels(
                    &self.gfx, self.screen_size,
                    Some("back"),
                    if has { Some("more") } else { None },
                    None,
                );
                self.gfx.flush().ok();
            }
            VaultMode::PhotoList => {
                if self.photo_cache.is_empty() {
                    self.clear_area();
                    crate::theme::heading(&self.gfx, self.screen_size, "PHOTOS");
                    crate::theme::list(
                        &self.gfx, self.screen_size, self.item_height,
                        &self.photo_cache, self.photo_cursor, "no photos yet",
                        crate::theme::ListStyle::Numbered,
                        None,
                        crate::theme::Repaint::All,
                    );
                    crate::theme::button_labels(
                        &self.gfx, self.screen_size, Some("back"), None, None,
                    );
                } else {
                    self.draw_photo_grid();
                    crate::theme::button_labels(
                        &self.gfx, self.screen_size, Some("back"), Some("more"), Some("view"),
                    );
                }
                self.gfx.flush().ok();
            }
            VaultMode::PhotoPreview | VaultMode::PhotoView => {
                let fresh = matches!(mode_at_entry, VaultMode::PhotoPreview);
                match self.pending_photo.as_ref() {
                    Some(bits) => {
                        self.gfx.bitmap(bits, None, None).ok();
                    }
                    None => {
                        self.clear_area();
                        crate::theme::heading(&self.gfx, self.screen_size, "NO PHOTO");
                    }
                }
                // A fresh shot is not stored yet, so it offers keep/retake; a stored one is
                // browsed with the arrows and only needs a way out.
                if fresh {
                    crate::theme::button_labels(
                        &self.gfx, self.screen_size,
                        Some("back"), Some("redo"), Some("save"),
                    );
                } else {
                    crate::theme::button_labels(
                        &self.gfx, self.screen_size, Some("back"), Some("more"), None,
                    );
                }
                self.gfx.flush().ok();
            }
            VaultMode::SettingsBling => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "BLING");
                // Built-ins only. A photo is set from the photos screen, where you can see
                // the picture you are choosing rather than a filename.
                let rows: Vec<String> =
                    BUILTIN_IMAGES.iter().map(|s| s.to_string()).collect();
                crate::theme::list(
                    &self.gfx, self.screen_size, self.item_height,
                    &rows, self.bling_cursor, "no images",
                    crate::theme::ListStyle::Select { marked: Some(self.standby_choice) },
                    None,
                    crate::theme::Repaint::All,
                );
                crate::theme::button_labels(
                    &self.gfx, self.screen_size, Some("back"), None, Some("pick"),
                );
                self.gfx.flush().ok();
            }
            VaultMode::SettingsBlinky => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "BLINKY");
                let rows: Vec<String> =
                    BLINKY_CHOICES.iter().map(|s| s.to_string()).collect();
                crate::theme::list(
                    &self.gfx, self.screen_size, self.item_height,
                    &rows, self.blinky_cursor, "no patterns",
                    crate::theme::ListStyle::Select { marked: Some(self.led_pattern) },
                    None,
                    crate::theme::Repaint::All,
                );
                // patterns need the carrier; say so rather than offering a dead control
                crate::theme::button_labels(
                    &self.gfx, self.screen_size, Some("back"), None, Some("pick"),
                );
                self.gfx.flush().ok();
            }

            VaultMode::Idle => {
                let now = self.tt.elapsed_ms();
                // Draw the chosen standby image, and only that one. It used to alternate the
                // user's image with the S-CAM logo, and both built-in choices cleared
                // user_bitmap, so picking DEFCON left nothing to alternate with and the logo
                // was drawn unconditionally.
                //
                // Repaint ONLY on change. This screen animates, so redraw runs continuously;
                // blitting the whole panel every time saturates the graphics server and the
                // badge never finishes booting. The slow edge is kept as a self-heal so the
                // image comes back if something else paints over it, at one blit per 3s
                // rather than one per tick.
                let standby_now = (self.standby_choice, self.user_bitmap.is_some());
                let edge = (now / 3000) % 2 == 0;
                if mode_at_entry != self.last_mode
                    || self.standby_drawn != Some(standby_now)
                    || self.edge != edge
                {
                    // bitmap_diffusion, not bitmap: the plain BaosecBitmap path is handled in
                    // bao-video without the display timeout guard that the diffuse path has,
                    // and swapping the stored-image draw onto it is what stopped the badge
                    // booting. The diffuse call is the one this screen has always used.
                    if self.standby_choice == DEFCON_IMAGE {
                        self.gfx.bitmap_diffusion(&bitmaps::dc_logo::BITMAP, None, None).ok();
                    } else if let Some(bitmap) = self.user_bitmap.as_ref() {
                        self.gfx.bitmap_diffusion(bitmap, None, None).ok();
                    } else {
                        self.gfx.bitmap_diffusion(&bitmaps::scam_logo::BITMAP, None, None).ok();
                    }
                    self.edge = edge;
                    self.standby_drawn = Some(standby_now);
                }

                // flag a badge mismatch, mostly for diagnostics at the factory & at the show
                let mut tv = TextView::new(
                    Gid::dummy(),
                    TextBounds::CenteredTop(Rectangle::new(Point::new(0, 127 - 12), Point::new(110, 128))),
                );
                tv.invert = true;
                tv.margin = Point::new(1, 1);
                tv.style = crate::theme::FONT;
                tv.draw_border = false;
                // have to move this out or we lock up
                let badge_type = self.global_config.as_ref().unwrap().lock().unwrap().badge_type();
                match self.global_config.as_ref().unwrap().lock().unwrap().attachment_state() {
                    AttachState::Mismatched => {
                        write!(tv, "Mismatched!").ok();
                        self.gfx.draw_textview(&mut tv).ok();
                    }
                    AttachState::FirstMate => {
                        write!(tv, "Attach: {:?}", badge_type).ok();
                        self.gfx.draw_textview(&mut tv).ok();
                    }
                    _ => {
                        // do nothing
                    }
                }

                // check battery voltage
                if !self.batt_polled
                    && (now / 1000) % 4 == 0
                    && !self.global_config.as_ref().unwrap().lock().unwrap().is_plugged_in()
                {
                    let voltage_code = self.adc.read_raw(
                        bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc3),
                        Some(8),
                    );
                    let vbat_mv =
                        ((bao1x_hal::udma::Adc::raw_to_voltage(voltage_code) * 1000.0f32) / 0.318f32) as u32;
                    if vbat_mv < LOWBATT_THRESH_MV {
                        self.gfx.bitmap(&bitmaps::lowbatt::BITMAP, None, None).ok();
                        let mut msg = TextView::new(
                            Gid::dummy(),
                            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 0), Point::new(127, 16))),
                        );
                        write!(msg, "Batt: {} mV", vbat_mv).ok();
                        msg.draw_border = false;
                        msg.clear_area = false;
                        msg.ellipsis = true;
                        msg.invert = true;
                        self.gfx.draw_textview(&mut msg).unwrap();

                        if let Some(since) = self.low_batt_since {
                            if std::time::Instant::now().duration_since(since).as_secs() > LOWBATT_TIMEOUT_S {
                                self.global_config.as_ref().unwrap().lock().unwrap().power_off();
                            }
                        } else {
                            self.low_batt_since = Some(std::time::Instant::now())
                        }
                    } else {
                        self.low_batt_since = None;
                    }
                    // only poll once per transition into this state
                    self.batt_polled = true;
                } else {
                    self.batt_polled = false;
                }

                // indicate BIO hacks
                if self.bio_loaded && (now / 1000) % 2 == 0 {
                    let mut tv = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredTop(Rectangle::new(
                            Point::new(0, 127 - 12),
                            Point::new(128, 128),
                        )),
                    );
                    tv.invert = true;
                    tv.margin = Point::new(1, 1);
                    tv.style = crate::theme::FONT;
                    tv.draw_border = false;
                    write!(tv, "BIO ACTIVE").ok();
                    self.gfx.draw_textview(&mut tv).ok();
                }

                // rate feedback
                let rate = self.global_config.as_ref().unwrap().lock().unwrap().get_mutation_rate();
                match rate {
                    MutationRate::Radioactive | MutationRate::Apocalyptic | MutationRate::Elevated => {
                        let mut msg = TextView::new(
                            Gid::dummy(),
                            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 0), Point::new(127, 10))),
                        );
                        msg.style = crate::theme::FONT;
                        msg.invert = true;
                        if rate == MutationRate::Elevated {
                            write!(msg, "elevated").ok();
                        } else if rate == MutationRate::Radioactive {
                            write!(msg, "RADIOACTIVE").ok();
                        } else if rate == MutationRate::Apocalyptic {
                            write!(msg, "~APOCALYPTIC~").ok();
                        }
                        msg.draw_border = false;
                        msg.clear_area = false;
                        msg.ellipsis = true;
                        msg.invert = true;
                        self.gfx.draw_textview(&mut msg).unwrap();
                    }
                    _ => {}
                }
            }
            // Both QR screens draw the same thing: a code filling the panel with its text
            // scrolling underneath. They differ only in what was encoded and where LEFT
            // goes, and both of those are settled elsewhere - so they share the drawing
            // rather than keeping a second copy of it. ABOUT used to have its own arm that
            // drew a heading and nothing else, on a mode flagged as animating, so it
            // repainted an empty panel several times a second.
            VaultMode::ShowBookmarkQr { quantum } | VaultMode::AboutQr { quantum } => {
                let about = matches!(mode_at_entry, VaultMode::AboutQr { .. });
                if let Some(code) = &self.qr_override {
                    if quantum & 7 == 0 {
                        self.clear_area();
                        let width = code.width();
                        let modules: Vec<bool> =
                            code.to_colors().into_iter().map(|c| c != Color::Light).collect();
                        // Bound the code so it stops short of the caption strip. It is sized
                        // from the width and drawn square, so at full width it fills the panel
                        // and the caption's background then covers its bottom rows.
                        let fit = (self.screen_size.y - crate::theme::LABEL_BAR_H) as usize;
                        self.gfx.render_qr(&modules, width, Point::new(0, 0), fit).ok();
                    }
                    // Caption the code with the URL it encodes. Redrawn on every other tick so
                    // a URL too long for the panel scrolls; the TextView refills its own box,
                    // so repainting in place does not smear over the code above it.
                    if quantum & 1 == 0 {
                        if let Some(url) = self.qr_caption.clone() {
                            let mut tv = TextView::new(
                                Gid::dummy(),
                                TextBounds::BoundingBox(Rectangle::new(
                                    Point::new(0, self.screen_size.y - crate::theme::LABEL_BAR_H),
                                    Point::new(self.screen_size.x, self.screen_size.y),
                                )),
                            );
                            tv.invert = true;
                            tv.margin = Point::new(0, 1);
                            tv.style = crate::theme::FONT;
                            tv.draw_border = false;
                            write!(tv, "{}", crate::theme::marquee(&url, quantum as u64 * 125, 18, 0)).ok();
                            self.gfx.draw_textview(&mut tv).ok();
                        }
                    }
                    // Advance the tick that drives the redraw cadence and the caption scroll,
                    // staying on whichever of the two screens is showing.
                    *self.mode.lock().unwrap() = if about {
                        VaultMode::AboutQr { quantum: quantum + 1 }
                    } else {
                        VaultMode::ShowBookmarkQr { quantum: quantum + 1 }
                    };
                } else {
                    // if no code, go back to idle mode
                    *self.mode.lock().unwrap() = VaultMode::Idle;
                }
            }
            VaultMode::Totp => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "2FA DIGITS");
                // check if time is set
                if chrono::Local::now().year() < 2026 {
                    // time is not set, print a warning to set time instead of the regular UI
                    let mut tv = TextView::new(Gid::dummy(), TextBounds::CenteredTop(TotpLayout::list_box()));
                    tv.invert = true;
                    tv.margin = Point::new(0, 0);
                    tv.style = crate::theme::FONT;
                    tv.draw_border = false;
                    write!(tv, "TIME IS NOT SET.\nSCAN A QR CODE TO SET IT.")
                        .ok();
                    self.gfx.draw_textview(&mut tv).ok();
                    self.gfx.flush().ok();
                    return;
                }
                // decorative box around code
                let mut totp_box = TotpLayout::totp_box();
                totp_box.border.style = DrawStyle::new(PixelColor::Dark, PixelColor::Light, 1);
                self.gfx.draw_rounded_rectangle(totp_box).ok();

                // the TOTP code
                let mut tv = TextView::new(
                    Gid::dummy(),
                    TextBounds::CenteredTop(
                        TotpLayout::totp_box().border.translate_chain(TotpLayout::totp_font_vmargin()),
                    ),
                );
                tv.invert = true;
                tv.margin = Point::new(0, 0);
                tv.style = TotpLayout::totp_font();
                tv.draw_border = false;

                if self.totp_code.is_none() && self.display_list.len() > 0 {
                    // this handles initial population of the field
                    self.update_selected_totp_code();
                }

                match &self.totp_code {
                    Some(code) => {
                        write!(tv, "{}", code).ok();
                    }
                    _ => {
                        write!(tv, "******").ok();
                    }
                }
                self.gfx.draw_textview(&mut tv).expect("couldn't draw text");

                // list of codes to pick from
                if self.totp_code.is_some() {
                    self.display_list.draw(TotpLayout::timer_box().br().y);
                } else {
                    let mut tv = TextView::new(Gid::dummy(), TextBounds::CenteredTop(TotpLayout::list_box()));
                    tv.invert = true;
                    tv.margin = Point::new(0, 0);
                    tv.style = crate::theme::FONT;
                    tv.draw_border = false;
                    write!(tv, "NO 2FA DIGITS.\nADD THEM BY SCANNING A QR CODE.").ok();
                    self.gfx.draw_textview(&mut tv).ok();
                }

                // draw the timer element
                let mut object_list = ObjectList::new();
                let mut timer_box = TotpLayout::timer_box();
                timer_box.style = DrawStyle::new(PixelColor::Dark, PixelColor::Light, 1);
                object_list.push(ClipObjectType::Rect(timer_box)).unwrap();

                // draw the duration bar
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|duration| duration.as_millis())
                    .expect("couldn't get time as millis");

                // manage the epoch as well
                let epoch = (current_time / (30 * 1000)) as u64;
                if self.last_epoch != epoch {
                    self.last_epoch = epoch;
                    self.update_selected_totp_code();
                }

                let mut timer_remaining = TotpLayout::timer_box();
                let delta = (current_time - (self.last_epoch as u128 * 30 * 1000)) as isize;
                let width = timer_remaining.width() as isize;
                let delta_width = (delta * width * 128) / (30 * 128 * 1000);
                timer_remaining.br = Point::new(width - delta_width, timer_remaining.br().y);
                timer_remaining.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                object_list.push(ClipObjectType::Rect(timer_remaining)).unwrap();
                self.gfx.draw_object_list(object_list).unwrap();
                crate::theme::button_labels(
                    &self.gfx, self.screen_size, Some("back"), Some("more"), Some("send"),
                );
                self.gfx.flush().ok();
            }
            VaultMode::Password => {
                self.clear_area();
                let screensize = self.screen_size;
                // handle empty database case
                if self.item_lists.lock().unwrap().filter_len(VaultMode::Password) == 0 {
                    log::debug!("no items");
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredBot(Rectangle::new(
                            Point::new(0, 0),
                            Point::new(screensize.x, screensize.y / 2 + self.item_height),
                        )),
                    );
                    box_text.draw_border = false;
                    box_text.clear_area = true;
                    box_text.invert = true;
                    box_text.style = crate::theme::FONT;
                    if self.filter.len() == 0 {
                        write!(box_text, "NO PASSWORDS.\nADD THEM BY SCANNING A QR CODE.").ok();
                    } else {
                        write!(box_text, "NOTHING MATCHING {}", &self.filter).ok();
                    }
                    self.gfx.draw_textview(&mut box_text).expect("couldn't post empty notification");
                    // "more" stays on an empty list: it carries "new", which is the only way
                    // to get the first record onto the badge.
                    // The empty state used to return here, before any labels were drawn, which
                    // left the screen with no way out and no indication there was one.
                    crate::theme::heading(&self.gfx, self.screen_size, "PASSWORDS");
                    crate::theme::button_labels(
                        &self.gfx, self.screen_size, Some("back"), Some("more"), None,
                    );
                    self.gfx.flush().ok();
                    return;
                }

                crate::theme::heading(&self.gfx, self.screen_size, "PASSWORDS");
                // ---- draw the top "detail info" about the selected password ----
                let mut insert_at = crate::theme::LABEL_BAR_H;
                if let Some(entry) = self.get_selected_item() {
                    log::debug!("rendering entry {:?}", entry);
                    // draw more data about the selected item
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredTop(Rectangle::new(
                            Point::new(0, insert_at),
                            Point::new(screensize.x, insert_at + self.item_height * 3),
                        )),
                    );
                    box_text.draw_border = false;
                    box_text.clear_area = false;
                    box_text.ellipsis = true;
                    box_text.style = self.style;
                    box_text.invert = true;
                    // line 1
                    write!(box_text, "{}/{} [{}]", &entry.name(), &entry.extra, entry.count).ok();
                    self.gfx.draw_textview(&mut box_text).unwrap();
                    insert_at += box_text.bounds_computed.unwrap().height() as isize;
                } else {
                    // draw just the empty rectangle around the top area if nothing is selected
                    self.gfx
                        .draw_rectangle(Rectangle::new_coords_with_style(
                            0,
                            0,
                            screensize.x,
                            self.item_height * 2,
                            DrawStyle {
                                fill_color: Some(PixelColor::Dark),
                                stroke_color: Some(PixelColor::Light),
                                stroke_width: 2,
                            },
                        ))
                        .ok();
                    log::error!("Couldn't retrieve password info to render top area");
                    insert_at = crate::theme::LABEL_BAR_H + self.item_height * 2;
                };
                self.display_list.draw(insert_at);
                crate::theme::button_labels(
                    &self.gfx, self.screen_size, Some("back"), Some("more"), Some("type"),
                );
                self.gfx.flush().ok();
            }// _ => unimplemented!(),
            VaultMode::BookmarkList => {
                // A marquee tick moves one row. Clearing and repainting the whole screen
                // several times a second to animate it flashes - the fault that made the
                // photo grid unusable - so the furniture is drawn when the screen or the
                // cursor changes, and after that only the moving row repaints itself.
                let full = self.list_quantum == 0;
                if full {
                    self.clear_area();
                    crate::theme::heading(&self.gfx, self.screen_size, "QR COLLECTION");
                }
                // Just the URL. A bookmark is only ever created by scanning a QR code, and
                // save_bookmark stores an empty label every time - there is no path that sets
                // one, so the "label: url" form this used to build never once ran.
                let rows: Vec<String> =
                    self.bookmark_cache.iter().map(|(_, url, _)| url.clone()).collect();
                crate::theme::list(
                    &self.gfx, self.screen_size, self.item_height,
                    &rows, self.bookmark_cursor, "no codes yet",
                    crate::theme::ListStyle::Numbered,
                    Some(self.tt.elapsed_ms().saturating_sub(self.list_focus_ms)),
                    if full { crate::theme::Repaint::All } else { crate::theme::Repaint::FocusedRow },
                );
                // drives the focused row's marquee; reset whenever the cursor or screen changes
                self.list_quantum = self.list_quantum.wrapping_add(1);
                if full {
                    let has = !rows.is_empty();
                    crate::theme::button_labels(
                        &self.gfx, self.screen_size,
                        Some("back"),
                        if has { Some("more") } else { None },
                        if has { Some("show") } else { None },
                    );
                }
                self.gfx.flush().ok();
            }
            VaultMode::ShowUrl => {
                // Display the scanned URL with a header row and wrapped text
                self.gfx.clear().ok();
                // Header: "URL" label in an inverted row at top
                crate::theme::heading(&self.gfx, self.screen_size, "URL");
                // URL text: left-aligned, below the heading, stopping short of the button bar
                if let Some(url) = &self.show_url {
                    let mut tv = TextView::new(
                        Gid::dummy(),
                        TextBounds::BoundingBox(Rectangle::new(
                            Point::new(0, crate::theme::LABEL_BAR_H),
                            Point::new(127, 127 - crate::theme::LABEL_BAR_H),
                        )),
                    );
                    tv.style = crate::theme::FONT;
                    tv.draw_border = false;
                    tv.invert = true; // white on black, like every other screen
                    tv.ellipsis = true; // truncation indicator if URL exceeds display capacity
                    write!(tv, "{}", url).ok();
                    self.gfx.draw_textview(&mut tv).ok();
                }
                crate::theme::button_labels(
                    &self.gfx, self.screen_size,
                    Some("back"), Some("redo"), Some("save"),
                );
            }
        }
        self.gfx.flush().ok();
        self.last_mode = (*self.mode.lock().unwrap()).clone();
    }

    /// Returns `true` if in longpress state. Only call this once per key hit input.
    pub(crate) fn manage_longpress(&mut self) -> bool {
        let now = self.tt.elapsed_ms();
        if now - self.last_key_time > KEYUP_DELAY_MS {
            self.start_hold_time = now;
        }
        self.last_key_time = now;
        now - self.start_hold_time > FAST_SCROLL_DELAY_MS
    }

    pub(crate) fn test_string(&mut self, s: &str) {
        let old =
            std::mem::replace(&mut self.factory_test, FactoryTestState::Error("Transitioning".to_string()));
        self.factory_test = old.handle_input(None, Some(s.to_owned()), None, false);
        self.redraw();
    }

    // Called when the jig indicates that the factory test is ready
    pub(crate) fn jig_ready(&mut self) {
        let old =
            std::mem::replace(&mut self.factory_test, FactoryTestState::Error("Transitioning".to_string()));
        self.factory_test = old.handle_input(None, None, None, true);
        self.redraw();
    }

    /// Grab the panel as a photo and show it for approval.
    ///
    /// Must run before anything else redraws: the frame lives in the panel buffer, and the
    /// camera has already stopped by the time this is called. Nothing is stored yet - the
    /// point of the preview is that the shot can be rejected.
    pub(crate) fn begin_photo_preview(&mut self) {
        match self.gfx.acquire_frame() {
            Ok(capture) if capture.ok => {
                self.pending_photo = Some(capture.bits);
                // a fresh capture is not any stored photo
                self.photo_loaded_key = None;
                *self.mode.lock().unwrap() = VaultMode::PhotoPreview;
            }
            Ok(_) => log::warn!("frame capture reported failure"),
            Err(e) => log::warn!("frame capture failed: {:?}", e),
        }
    }


    /// Store the held photo. Returns false when the store is full, so the screen can say so
    /// rather than silently dropping the shot.
    fn keep_pending_photo(&mut self) -> bool {
        let Some(bits) = self.pending_photo else { return false };
        let stored = crate::storage::photo_store(&self.pddb.borrow(), &bits);
        match stored {
            Some(key) => {
                log::info!("photo stored as {}", key);
                self.pending_photo = None;
        self.photo_loaded_key = None;
                self.load_photos();
                true
            }
            None => false,
        }
    }

    /// Reopen the camera. main.rs owns the camera path because AcquireQr is a blocking
    /// scalar, so ask it rather than trying to drive the camera from here.
    ///
    /// Used by both RETAKE and SAVE: after keeping a shot you are still taking photos, so
    /// handing back the live camera beats dropping you into a list.
    fn reopen_camera(&mut self) {
        self.pending_photo = None;
        self.photo_loaded_key = None;
        *self.mode.lock().unwrap() = VaultMode::Idle;
        xous::send_message(
            self.main_cid,
            xous::Message::new_scalar(VaultOp::ScanUrl.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }

    /// Ask for the QR of the bookmark under the cursor.
    ///
    /// Routed through ActionManager rather than encoded here so browsing uses the same
    /// validation path as opening one from the list - it is the only thing that checks the
    /// stored URL before it becomes a QR code someone may scan.
    fn request_bookmark_qr(&mut self) {
        if let Some((key, _, _)) = self.bookmark_cache.get(self.bookmark_cursor) {
            let ipc_key = crate::IpcString { s: key.clone() };
            if let Ok(buf) = xous_ipc::Buffer::into_buf(ipc_key) {
                buf.lend(self.actions_conn, crate::actions::ActionOp::BookmarkSelected.to_u32().unwrap())
                    .ok();
            }
        }
    }

    /// Draw the photos as a 2x2 grid of 56px thumbnails.
    ///
    /// Composed into one frame and sent with `bitmap`, not `bitmap_diffusion`. Diffusion
    /// drives the panel itself over about ten animation steps, so it took over the whole
    /// display - button labels included - and flashed on every focus change. `bitmap` writes
    /// the back buffer and the panel updates once, on the flush at the end of the redraw,
    /// like every other screen.
    ///
    /// The frame starts filled with ones: a zero-filled frame renders WHITE on this panel, so
    /// zeroing it put every thumbnail on a white background. Source bits are copied verbatim,
    /// set and cleared, rather than only ever set.
    ///
    /// No heading here. Two rows plus the button bar already fill the panel, so a heading
    /// would cost a whole row of thumbnails.
    ///
    /// 54px cells are the largest that fit: two of them, plus a 4px gap and a pixel of
    /// bracket clearance top and bottom, comes to exactly the 114 rows above the button bar.
    fn draw_photo_grid(&mut self) {
        const CELL: usize = 54;
        const COLS: usize = 2;
        const ROWS: usize = 2;
        /// gap between thumbnails
        const GAP: usize = 4;
        /// clearance between a thumbnail and its focus brackets
        const PAD: usize = 1;
        const PITCH: usize = CELL + GAP;
        const X0: usize = (128 - (CELL * COLS + GAP)) / 2;
        /// first row starts one pixel down so the brackets above it are not clipped
        const Y0: usize = PAD;

        let page = self.photo_cursor / (COLS * ROWS);
        let first = page * COLS * ROWS;

        let mut frame = [u32::MAX; 512];
        for slot in 0..COLS * ROWS {
            let Some(key) = self.photo_cache.get(first + slot) else { break };
            let Some(src) = crate::storage::photo_get(&self.pddb.borrow(), key) else { continue };
            let ox = X0 + (slot % COLS) * PITCH;
            let oy = Y0 + (slot / COLS) * PITCH;
            // nearest neighbour: one source pixel per destination pixel. There is nothing to
            // average in a 1bpp image.
            for dy in 0..CELL {
                let sy = dy * 128 / CELL;
                for dx in 0..CELL {
                    let sx = dx * 128 / CELL;
                    let si = sx + sy * 128;
                    let di = (ox + dx) + (oy + dy) * 128;
                    if (src[si >> 5] >> (si & 31)) & 1 != 0 {
                        frame[di >> 5] |= 1 << (di & 31);
                    } else {
                        frame[di >> 5] &= !(1 << (di & 31));
                    }
                }
            }
        }
        self.gfx.bitmap(&frame, None, None).ok();

        // Same focus mark as every list, sitting PAD outside the thumbnail so there is a
        // visible gap rather than brackets drawn over the picture.
        let slot = self.photo_cursor - first;
        let cx = (X0 + (slot % COLS) * PITCH) as isize;
        let cy = (Y0 + (slot / COLS) * PITCH) as isize;
        let pad = PAD as isize;
        ux_api::widgets::scroll::draw_corner_brackets(
            &self.gfx,
            Rectangle::new(
                Point::new(cx - pad, cy - pad),
                Point::new(cx + CELL as isize - 1 + pad, cy + CELL as isize - 1 + pad),
            ),
        );

        // Say whether there are more pages. The grid shows four at a time and silently paged
        // on the cursor, so eight photos and four looked exactly the same until you walked
        // off the end of the screen. The centred grid leaves 8px clear on each side, so the
        // bar goes in that margin rather than over a thumbnail.
        if self.photo_cache.len() > COLS * ROWS {
            crate::theme::scrollbar(
                &self.gfx,
                self.screen_size.x,
                Y0 as isize,
                (Y0 + ROWS * PITCH - GAP) as isize,
                first,
                COLS * ROWS,
                self.photo_cache.len(),
            );
        }
    }

    /// Wrap a captured frame in a 1bpp BMP.
    ///
    /// BMP because it can be built with a fixed 62-byte header and no compression, so the
    /// badge emits something a browser opens directly - PNG would need zlib and a CRC.
    ///
    /// Height is negative so rows read top-down; BMP is bottom-up by default. Bits are
    /// reversed within each byte: the frame stores the leftmost pixel in the LSB, BMP wants it
    /// in the MSB. The palette is white then black, matching the frame where 0 is lit.
    fn photo_to_bmp(bits: &[u32; 512]) -> Vec<u8> {
        const W: usize = 128;
        const ROW: usize = W / 8; // already a multiple of 4, so no padding needed
        let pixels = ROW * W;
        let offset: u32 = 14 + 40 + 8;
        let size: u32 = offset + pixels as u32;

        let mut b = Vec::with_capacity(size as usize);
        b.extend_from_slice(b"BM");
        b.extend_from_slice(&size.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&offset.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
        b.extend_from_slice(&(W as i32).to_le_bytes());
        b.extend_from_slice(&(-(W as i32)).to_le_bytes()); // negative: top-down
        b.extend_from_slice(&1u16.to_le_bytes()); // planes
        b.extend_from_slice(&1u16.to_le_bytes()); // bits per pixel
        b.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        b.extend_from_slice(&(pixels as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes()); // colours used
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]); // index 0: white
        b.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // index 1: black

        let mut rows = vec![0u8; pixels];
        for y in 0..W {
            for x in 0..W {
                let i = x + y * W;
                if (bits[i >> 5] >> (i & 31)) & 1 != 0 {
                    rows[y * ROW + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        b.extend_from_slice(&rows);
        b
    }

    /// Standard base64. Written out rather than pulled in as a dependency - it is fifteen
    /// lines and the app is watching its page budget.
    fn base64(data: &[u8]) -> String {
        const SET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(SET[(n >> 18) as usize & 63] as char);
            out.push(SET[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 { SET[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if chunk.len() > 2 { SET[n as usize & 63] as char } else { '=' });
        }
        out
    }

    /// Type the alphabet at a given delay, to find where the host starts losing characters.
    ///
    /// Lower case then upper case: the upper case half exercises the shift modifier, which is
    /// sent in the same report as the letter, and is the part most likely to break when the
    /// pacing is too tight.
    /// Render the shown photo as ASCII art.
    ///
    /// One character per 1x2 block of pixels, so 128 columns by 64 rows. Terminal cells are
    /// about twice as tall as they are wide, so that comes out roughly square; one character
    /// per pixel would be twice as tall as it should be and twice as long to type.
    ///
    /// The four characters mark where the ink is within the cell: a high mark for the top
    /// pixel, a low one for the bottom, a solid one for both.
    fn ascii_art(bits: &[u32; 512]) -> String {
        const W: usize = 128;
        // Byte table rather than a match on a tuple, and bytes rather than chars: the output
        // is ASCII by construction, and the leaner shape keeps this function small. Adding it
        // as originally written stopped the badge booting even though nothing calls it at
        // startup - this app sits close enough to the edge that code layout matters.
        const MARKS: [u8; 4] = [b' ', b'"', b'.', b'#'];
        let mut out = vec![0u8; 0];
        out.reserve(W * 64 + 64);
        for row in 0..64usize {
            for x in 0..W {
                let top = x + (row * 2) * W;
                let bot = top + W;
                let t = (bits[top >> 5] >> (top & 31)) & 1;
                let b = (bits[bot >> 5] >> (bot & 31)) & 1;
                out.push(MARKS[(t | (b << 1)) as usize]);
            }
            out.push(b'\n');
        }
        String::from_utf8(out).unwrap_or_default()
    }

    /// Type the shown photo to the host as a data URI.
    ///
    /// The badge has no way to hand over a file: mass storage is the boot ROM's, not the
    /// app's, and this USB core is fixed to keyboard plus FIDO. So it types. A data URI means
    /// the result is openable as-is in a browser rather than needing a decoder.
    ///
    /// About 2800 characters, so it takes a while and goes wherever the host's focus is -
    /// hence the confirm, and the reminder to put the cursor somewhere first.
    /// How many photos are stored. Answers the serial console's `photo list`.
    pub(crate) fn photo_count(&mut self) -> usize {
        self.load_photos();
        self.photo_cache.len()
    }

    /// Export photo `index` without disturbing the screen or the cursor.
    ///
    /// The menu-driven export works on whatever the cursor is pointing at, which is right for
    /// someone standing at the badge and wrong for a host asking for a specific photo. This
    /// takes the index from the request, and reports over serial rather than on the display -
    /// nobody is looking at the badge when they are driving it from a terminal.
    pub(crate) fn export_photo_at(&mut self, index: usize, as_art: bool) -> bool {
        self.load_photos();
        let Some(key) = self.photo_cache.get(index).cloned() else {
            return false;
        };
        let Some(bits) = crate::storage::photo_get(&self.pddb.borrow(), &key) else {
            return false;
        };
        let text = if as_art {
            Self::ascii_art(&bits)
        } else {
            format!("data:image/bmp;base64,{}\n", Self::base64(&Self::photo_to_bmp(&bits)))
        };
        self.serial_out(&text);
        true
    }

    /// Type the shown photo to the host as keystrokes.
    ///
    /// Slower than serial by a wide margin - a report per keystroke, two per character, each
    /// waiting on the host's polling interval - but it needs nothing installed on the other
    /// end, which is the whole point of having it.
    pub(crate) fn type_photo(&mut self, as_art: bool) {
        let Some(bits) = self.pending_photo else {
            self.notify("NOTHING TO EXPORT");
            return;
        };
        let text = if as_art {
            Self::ascii_art(&bits)
        } else {
            format!("data:image/bmp;base64,{}\n", Self::base64(&Self::photo_to_bmp(&bits)))
        };

        // Small chunks, and not because of the page limit.
        //
        // The USB server handles one message at a time, and a send_str call does not return
        // until every keystroke in it has been typed. At 1024 characters that is ten to
        // twenty seconds during which nothing else can use USB at all - including the CDC
        // serial that carries the log - and the badge reliably fell over partway through a
        // photo. Sixty-four characters is about a second per call, so the server stays
        // responsive between them. The total time is unchanged; the stalls are not.
        //
        // Bytes rather than chars: both formats are ASCII by construction, and collecting
        // 8256 chars into a Vec<char> cost 33KB of heap on an app that is demand-paged out
        // of encrypted swap.
        const CHUNK: usize = 64;
        let bytes = text.as_bytes();
        let mut typed = 0;
        for part in bytes.chunks(CHUNK) {
            let s = match core::str::from_utf8(part) {
                Ok(s) => s,
                Err(_) => continue, // unreachable for ASCII, and not worth dying over
            };
            match self.usb_dev.send_str(s) {
                Ok(0) => {
                    self.notify("NO USB HOST");
                    return;
                }
                Ok(n) => typed += n,
                Err(e) => {
                    log::error!("HID photo type failed: {:?}", e);
                    self.notify("TYPE FAILED");
                    return;
                }
            }
        }
        if typed >= bytes.len() {
            self.notify("TYPED TO HOST");
        } else {
            log::warn!("typed {} of {} characters", typed, bytes.len());
            self.notify(&format!("TYPED {} OF {}", typed, bytes.len()));
        }
    }

    /// Push text out of the CDC serial port, a page at a time.
    ///
    /// Shared by the menu export and the host-driven one so the chunking, the flush and the
    /// log-quieting cannot drift apart between them.
    fn serial_out(&mut self, text: &str) -> bool {
        const CHUNK: usize = 3840; // usb-bao1x SERIAL_BINARY_BUFLEN, not re-exported
        let prior_level = log::max_level();
        log::set_max_level(log::LevelFilter::Warn);
        let data = text.as_bytes();
        let mut sent = 0;
        let mut ok = true;
        while sent < data.len() {
            let end = (sent + CHUNK).min(data.len());
            match self.usb_dev.serial_send(&data[sent..end]) {
                Ok(0) => {
                    ok = false;
                    break;
                }
                Ok(n) => {
                    sent += n;
                    self.usb_dev.serial_flush().ok();
                }
                Err(e) => {
                    log::error!("serial export failed: {:?}", e);
                    ok = false;
                    break;
                }
            }
        }
        self.usb_dev.serial_flush().ok();
        log::set_max_level(prior_level);
        ok
    }

    pub(crate) fn export_photo(&mut self, as_art: bool) {
        let Some(bits) = self.pending_photo else {
            self.notify("NOTHING TO EXPORT");
            return;
        };
        let text = if as_art {
            Self::ascii_art(&bits)
        } else {
            format!("data:image/bmp;base64,{}\n", Self::base64(&Self::photo_to_bmp(&bits)))
        };

        // Over the CDC serial port, not the keyboard.
        //
        // Typing it was never going to work. The HID path turns each character into keycodes,
        // and a report the endpoint is not ready for used to be discarded outright - which
        // lost characters and, when the lost report was a key-up, left the host repeating a
        // key. Measured at seven different delays, including 50ms, every run came back
        // incomplete and identical, so no amount of pacing was going to fix it.
        //
        // Serial carries bytes. It reports how many were accepted, so a short write can be
        // retried rather than silently dropped, and there is no keymap in the path at all.
        // Hand over at most one buffer's worth at a time. serial_send clamps the length it
        // *reports* to SERIAL_BINARY_BUFLEN but copies whatever it is given into a single
        // page, and then panics on the failure rather than returning it - on the main thread,
        // which takes the whole app down. Base64 of a photo fits in a page and ASCII art does
        // not, which is why one worked and the other crashed the badge.
        const CHUNK: usize = 3840; // usb-bao1x SERIAL_BINARY_BUFLEN, not re-exported

        // Quiet the log for the duration. The log and this export share a CDC interface, so
        // anything logged while the transfer is running is spliced into the middle of the
        // image - a menu selection logged mid-export put 770 bytes of "INFO:ux_api::menu"
        // through the middle of a photo. The receiver drops whole log lines, but one injected
        // mid-line splits a row of data and neither half looks like a log line any more.
        let prior_level = log::max_level();
        log::set_max_level(log::LevelFilter::Warn);

        let data = text.as_bytes();
        let mut sent = 0;
        while sent < data.len() {
            let end = (sent + CHUNK).min(data.len());
            match self.usb_dev.serial_send(&data[sent..end]) {
                Ok(0) => {
                    self.notify("NO USB HOST - NOTHING SENT");
                    log::set_max_level(prior_level);
                    return;
                }
                Ok(n) => {
                    sent += n;
                    // Push each chunk out rather than leaving it in the CDC transmit buffer.
                    // serial_send only writes into that buffer; without a flush the bytes sit
                    // there until some unrelated USB traffic happens to move them, which is
                    // why the port existed and nothing ever arrived.
                    self.usb_dev.serial_flush().ok();
                }
                Err(e) => {
                    log::warn!("serial export failed after {} bytes: {:?}", sent, e);
                    self.notify("EXPORT FAILED");
                    log::set_max_level(prior_level);
                    return;
                }
            }
        }
        self.usb_dev.serial_flush().ok();
        // the notification itself logs, so restore only once the bytes are out
        log::set_max_level(prior_level);
        self.notify("EXPORT DONE");
    }

    /// Ask main to open the photo actions menu. Menus are owned by the main loop, so this
    /// posts a message rather than drawing one here.
    /// Type the focused saved URL to the host over HID.
    ///
    /// Short strings only, which a URL is. Bulk data goes over serial instead - typing a
    /// photo out dropped characters at every delay that was measured.
    pub(crate) fn type_bookmark(&mut self) {
        let Some((_, url, _)) = self.bookmark_cache.get(self.bookmark_cursor) else {
            self.notify("NOTHING SELECTED");
            return;
        };
        let msg = crate::IpcString { s: url.clone() };
        match xous_ipc::Buffer::into_buf(msg) {
            Ok(buf) => {
                buf.lend(self.actions_conn, crate::actions::ActionOp::TypeOutUrl.to_u32().unwrap())
                    .ok();
            }
            Err(e) => log::error!("type_bookmark: IPC buffer error: {:?}", e),
        }
    }

    /// Delete the saved QR under the cursor. The caller asks first.
    pub(crate) fn delete_bookmark(&mut self) {
        let Some((key, _, _)) = self.bookmark_cache.get(self.bookmark_cursor).cloned() else {
            // Say so. Answering "yes" to a confirmation and getting silence back is
            // indistinguishable from a delete that worked.
            self.notify("NOTHING SELECTED");
            return;
        };
        let deleted = crate::storage::bookmark_delete(&self.pddb.borrow(), &key);
        if let Err(e) = deleted {
            log::warn!("could not delete bookmark {}: {:?}", key, e);
            self.notify("DELETE FAILED");
            return;
        }
        self.load_bookmarks();
        self.notify("DELETED");
    }

    /// Open the actions menu for the record under the cursor.
    ///
    /// Same shape as the photo actions: the middle button offers what you can do with the
    /// thing you are looking at. These used to live in a separate "Token Menu" reached by
    /// the jog press, which had no label anywhere and so was effectively undiscoverable.
    fn open_bookmark_actions(&mut self) {
        xous::send_message(
            self.main_cid,
            xous::Message::new_scalar(VaultOp::MenuBookmarkActions.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }

    fn open_record_actions(&mut self) {
        xous::send_message(
            self.main_cid,
            xous::Message::new_scalar(VaultOp::MenuRecordActions.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }

    fn open_photo_actions(&mut self) {
        xous::send_message(
            self.main_cid,
            xous::Message::new_scalar(VaultOp::MenuPhotoActions.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }

    /// Delete the photo under the cursor. The caller asks first.
    pub(crate) fn delete_photo(&mut self) {
        // Say something either way. This used to delete in silence and swallow the error in
        // silence too, so a failed delete and a successful one looked identical - and the
        // saved-QR screen next door already answers both cases.
        let Some(key) = self.photo_cache.get(self.photo_cursor).cloned() else {
            self.notify("NOTHING SELECTED");
            return;
        };
        let failed = crate::storage::photo_delete(&self.pddb.borrow(), &key)
            .inspect_err(|e| log::warn!("could not delete photo {}: {:?}", key, e))
            .is_err();
        self.load_photos();
        self.pending_photo = None;
        self.photo_loaded_key = None;
        // Set the mode before notifying: the notification repaints whatever is underneath
        // when it clears, and that should already be the list this deletion returns to.
        *self.mode.lock().unwrap() = VaultMode::PhotoList;
        self.notify(if failed { "DELETE FAILED" } else { "DELETED" });
    }

    /// Load the photo under the cursor if the grid has not already done so. The export and
    /// wallpaper actions both need the full-size bits, and the grid only holds thumbnails.
    pub(crate) fn ensure_photo_loaded(&mut self) {
        let Some(key) = self.photo_cache.get(self.photo_cursor).cloned() else { return };
        // Reload when the cursor has moved to a different photo. The test used to be simply
        // "is anything loaded?", which is true as soon as one photo has been opened - so
        // selecting a second photo and exporting it sent the first one's bits again.
        if self.pending_photo.is_some() && self.photo_loaded_key.as_deref() == Some(key.as_str())
        {
            return;
        }
        // Load the bits only. show_selected_photo would also switch to the full-screen mode,
        // so setting a wallpaper from the grid would have dumped you into the viewer.
        match crate::storage::photo_get(&self.pddb.borrow(), &key) {
            Some(bits) => {
                self.pending_photo = Some(bits);
                self.photo_loaded_key = Some(key);
            }
            None => log::warn!("photo {} could not be read", key),
        }
    }

    /// Make the photo under the cursor the standby image.
    ///
    /// Standby choices are indexed past the built-ins, so photo N is BUILTIN_IMAGES.len() + N.
    pub(crate) fn set_photo_as_bling(&mut self) {
        if self.photo_cache.get(self.photo_cursor).is_none() {
            return;
        }
        let choice = BUILTIN_IMAGES.len() + self.photo_cursor;
        if let Err(e) = crate::storage::set_standby_choice(&self.pddb.borrow(), choice) {
            log::warn!("could not persist standby image: {:?}", e);
        }
        self.apply_standby_choice(choice);
        self.notify("SET AS BLING");
    }

    /// Load the photo under the photos-list cursor for full-screen viewing.
    pub(crate) fn show_selected_photo(&mut self) {
        if let Some(key) = self.photo_cache.get(self.photo_cursor).cloned() {
            match crate::storage::photo_get(&self.pddb.borrow(), &key) {
                Some(bits) => {
                    self.pending_photo = Some(bits);
                    self.photo_loaded_key = Some(key);
                    *self.mode.lock().unwrap() = VaultMode::PhotoView;
                }
                None => log::warn!("photo {} could not be read", key),
            }
        }
    }

    /// Leave a submenu the way it was entered: back to the menu, not past it to standby.
    ///
    /// The app cannot raise the menu widget itself - main.rs owns it - so the way back is to
    /// return the menu key and let main.rs open it. Dropping to Idle instead skipped a level
    /// and made every submenu feel like a dead end.
    fn to_menu(&mut self) -> Option<char> {
        *self.mode.lock().unwrap() = VaultMode::Idle;
        Some('∴')
    }

    pub(crate) fn handle_key(&mut self, k: char) -> Option<char> {
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();
        if k == '🔼' {
            self.orientation = DisplayOrientation::Normal;
            self.redraw();
            return Some(k);
        } else if k == '🔽' {
            self.orientation = DisplayOrientation::UpsideDown;
            self.redraw();
            return Some(k);
        }
        log::debug!("handle_key: {:?}", mode_at_entry);
        let filtered_k = match mode_at_entry {
            VaultMode::Password => {
                let increment = if self.manage_longpress() { PAGE_INCREMENT } else { 1 };
                match k {
                    '↑' => {
                        for _ in 0..increment {
                            self.display_list.key_action('↑');
                        }
                    }
                    '↓' => {
                        for _ in 0..increment {
                            self.display_list.key_action('↓');
                        }
                    }
                    // LEFT is back on every screen; the menu already reaches 2fa digits
                    // directly, so the old left-types / right-switches pair is retired.
                    '←' => return self.to_menu(),
                    // Consumed here. This arm falls through to `Some(k)`, and the main loop
                    // reads a stray middle button as "open the camera" - so handling the key
                    // without also swallowing it opened the actions menu and the camera at
                    // once. The jog press goes the same way: leaked, the main loop opens the
                    // record actions itself, which is the right menu here only by luck and
                    // the wrong one on the screens next door.
                    '🔥' | '∴' => {
                        self.open_record_actions();
                        return None;
                    }
                    '→' => {
                        if let Some(item) = self.get_selected_item() {
                            // Report it, do not panic on it. This process is the whole UI, so
                            // an unwrap here takes the screen down - the same shape as the CTAP
                            // storage unwrap that read as an unexplained boot loop. The error
                            // is reachable in normal use: handle_autotype re-fetches from the
                            // PDDB, which fails if a basis has been unmounted since the list
                            // was drawn.
                            if let Err(e) = self.handle_autotype(item.guid, false) {
                                log::warn!("autotype failed: {}", e);
                                self.notify("AUTOTYPE FAILED");
                            }
                        }
                    }
                    // A '\u{0}' arm used to swap this screen for the 2fa one and back. Nothing
                    // sends that char - it is NUL, which the main loop also uses as its own
                    // "no key" sentinel - and the menu reaches both screens directly now.
                    _ => {}
                }
                Some(k)
            }
            VaultMode::Totp => {
                match k {
                    '↑' => {
                        self.display_list.key_action('↑');
                    }
                    '↓' => {
                        self.display_list.key_action('↓');
                    }
                    '←' => return self.to_menu(),
                    // see the Password arm: both the middle button and the jog press have to
                    // be consumed here rather than left to the main loop
                    '🔥' | '∴' => {
                        self.open_record_actions();
                        return None;
                    }
                    '→' => {
                        if let Some(code) = self.update_selected_totp_code() {
                            // ignore USB errors while sending code
                            self.usb_dev.send_str(&code).ok();
                        }
                    }
                    // see the Password arm: the '\u{0}' screen swap that sat here was dead
                    _ => {}
                }
                self.totp_code = None;
                Some(k)
            }
            VaultMode::Passkeys => {
                let mut leaving = false;
                match k {
                    '↑' | '↓' => {
                        self.passkey_cursor =
                            step_cursor(self.passkey_cursor, self.passkey_cache.len(), k == '↑')
                    }
                    '←' => leaving = true,
                    // Delete moved into the actions menu. As a bare button it removed a
                    // credential on one press with nothing to confirm it.
                    // Gated on there being something to act on: the label bar hides "more"
                    // when the list is empty, so opening the menu anyway made the middle
                    // button an unlabelled control onto entries that could not do anything.
                    '🔥' | '∴' => {
                        if !self.passkey_cache.is_empty() {
                            self.open_record_actions();
                        }
                    }
                    _ => {}
                }
                if leaving {
                    return self.to_menu();
                }
                self.redraw();
                None
            }
            VaultMode::PhotoList => {
                let mut leaving = false;
                match k {
                    '↑' | '↓' => {
                        self.photo_cursor =
                            step_cursor(self.photo_cursor, self.photo_cache.len(), k == '↑')
                    }
                    '←' => leaving = true,
                    // Gated on there being a photo: the label bar hides "more" and "view"
                    // on an empty list, so acting anyway made them unlabelled controls.
                    '🔥' | '∴' => {
                        if !self.photo_cache.is_empty() {
                            self.open_photo_actions();
                        }
                    }
                    '→' => {
                        if !self.photo_cache.is_empty() {
                            self.show_selected_photo();
                        }
                    }
                    _ => {}
                }
                if leaving {
                    return self.to_menu();
                }
                self.redraw();
                None
            }
            VaultMode::PhotoPreview => {
                match k {
                    // Discard and go back where the camera was opened from, which is the
                    // standby screen - the only place it can be started for photos. The shot
                    // was never stored, so there is nothing else to undo.
                    '←' => {
                        self.pending_photo = None;
        self.photo_loaded_key = None;
                        *self.mode.lock().unwrap() = VaultMode::Idle;
                    }
                    '🔥' => {
                        self.reopen_camera();
                        return None;
                    }
                    '→' => {
                        if self.keep_pending_photo() {
                            // straight back to the viewfinder - you are still taking photos
                            self.reopen_camera();
                            return None;
                        }
                        self.pending_photo = None;
        self.photo_loaded_key = None;
                        self.notify("PHOTO STORE FULL");
                    }
                    _ => {}
                }
                self.redraw();
                None
            }
            VaultMode::PhotoView => {
                match k {
                    '←' => {
                        self.pending_photo = None;
        self.photo_loaded_key = None;
                        *self.mode.lock().unwrap() = VaultMode::PhotoList;
                    }
                    // browse without going back to the list
                    '↑' | '↓' => {
                        self.photo_cursor =
                            step_cursor(self.photo_cursor, self.photo_cache.len(), k == '↑');
                        self.show_selected_photo();
                    }
                    // Middle opens the actions menu on both screens. Everything destructive
                    // or persistent lives in there, so no single press changes anything.
                    // The jog press does the same rather than falling through to the main
                    // loop, which would offer the password-record actions instead.
                    '🔥' | '∴' => self.open_photo_actions(),
                    _ => {}
                }
                self.redraw();
                None
            }
            VaultMode::SettingsBling => {
                let mut leaving = false;
                let count = BUILTIN_IMAGES.len();
                match k {
                    '↑' | '↓' => self.bling_cursor = step_cursor(self.bling_cursor, count, k == '↑'),
                    '←' => leaving = true,
                    '→' => {
                        let choice = self.bling_cursor;
                        if let Err(e) =
                            crate::storage::set_standby_choice(&self.pddb.borrow(), choice)
                        {
                            log::warn!("could not persist standby image: {:?}", e);
                        }
                        self.apply_standby_choice(choice);
                    }
                    _ => {}
                }
                if leaving {
                    return self.to_menu();
                }
                self.redraw();
                None
            }
            VaultMode::SettingsBlinky => {
                let mut leaving = false;
                match k {
                    '↑' | '↓' => {
                        self.blinky_cursor =
                            step_cursor(self.blinky_cursor, BLINKY_CHOICES.len(), k == '↑')
                    }
                    '←' => leaving = true,
                    '→' => {
                        // Always send it. This used to be gated on an attachment probe, which
                        // meant a wrong or stale probe silently swallowed the selection with
                        // no way to tell that from a pattern that simply did not render. The
                        // ring is on the carrier; with no carrier the write just goes nowhere.
                        self.led_pattern = self.blinky_cursor;
                        if let Some(config) = self.global_config.as_ref() {
                            config.lock().unwrap().set_led_pattern(self.blinky_cursor);
                        }
                        if let Err(e) = crate::storage::set_blinky_choice(
                            &self.pddb.borrow(), self.blinky_cursor,
                        ) {
                            log::warn!("could not persist LED pattern: {:?}", e);
                        }
                    }
                    _ => {}
                }
                if leaving {
                    return self.to_menu();
                }
                self.redraw();
                None
            }
            VaultMode::Idle => match k {
                // LEFT - show the default bookmark as a QR, press again to dismiss.
                // "Default" is set explicitly from the bookmarks list; nothing defined it before.
                // LEFT opens the menu. It used to show the "default" bookmark as a QR, but
                // nothing in the UI ever set a default, so it was a button that did nothing on
                // most badges. The QR collection reaches every saved code.
                // LEFT opens the menu at the top. Returned as itself rather than the menu
                // key because a screen's BACK returns that, and BACK has to keep its
                // place in the list while opening from standby starts at the first item.
                '←' => Some(k),
                // MIDDLE - open the camera. Routed through the main loop because
                // ActionOp::AcquireQr is a blocking scalar and the key path cannot send one.
                '🔥' => Some(k),
                // RIGHT cycles the LED pattern. Not gated on an attachment probe: a wrong or
                // stale probe silently swallowed the press, which is indistinguishable from a
                // pattern that did not render. With no carrier the write goes nowhere.
                '→' => {
                    self.led_pattern = (self.led_pattern + 1) % (LED_PATTERN_COUNT + 1);
                    if let Some(config) = self.global_config.as_ref() {
                        config.lock().unwrap().set_led_pattern(self.led_pattern);
                    }
                    // Persist it, and move the settings cursor with it. Cycling from standby
                    // used to change the ring for this boot only, while picking the very same
                    // pattern under settings > blinky kept it - so the two controls disagreed,
                    // and the settings list went on marking a pattern that was no longer on.
                    // No notification: the ring itself is the feedback, and a 1.2s modal
                    // between presses would make cycling unusable.
                    self.blinky_cursor = self.led_pattern;
                    if let Err(e) =
                        crate::storage::set_blinky_choice(&self.pddb.borrow(), self.led_pattern)
                    {
                        log::warn!("could not persist LED pattern: {:?}", e);
                    }
                    None
                }
                _ => Some(k),
            },

            VaultMode::BookmarkList => {
                match k {
                    '↑' | '↓' => {
                        self.bookmark_cursor =
                            step_cursor(self.bookmark_cursor, self.bookmark_cache.len(), k == '↑');
                        // a newly focused row holds still before it starts scrolling
                        self.list_quantum = 0;
                        self.list_focus_ms = self.tt.elapsed_ms();
                    }
                    '←' => {
                        // back to the menu this was opened from, not past it to standby
                        return self.to_menu();
                    }
                    // Consumed here. This arm falls through to `Some(k)`, and the main loop
                    // reads a stray middle button as "open the camera" - so handling the key
                    // without also swallowing it opened the actions menu and the camera at
                    // once. The jog press has to be swallowed for a different reason: unhandled
                    // it reached the main loop, which reads a bare '∴' as "open the record
                    // actions" and offered new/edit/delete/filter for a PASSWORD record on a
                    // screen full of URLs.
                    '🔥' | '∴' => {
                        if !self.bookmark_cache.is_empty() {
                            self.open_bookmark_actions();
                        }
                        return None;
                    }
                    '→' => {
                        // select highlighted bookmark → trigger QR render via ActionManager
                        if let Some((key, _, _)) = self.bookmark_cache.get(self.bookmark_cursor) {
                            let key = key.clone();
                            let ipc_key = crate::IpcString { s: key };
                            if let Ok(buf) = xous_ipc::Buffer::into_buf(ipc_key) {
                                buf.lend(
                                    self.actions_conn,
                                    crate::actions::ActionOp::BookmarkSelected
                                        .to_u32()
                                        .unwrap(),
                                )
                                .ok();
                            }
                        }
                    }
                    _ => {}
                }
                Some(k)
            }
            // Any key leaves About. Without this arm it fell through to the catch-all,
            // which returns the key without changing mode - the screen had no exit at all.
            // Drop the code on the way out: it is shared with the saved-QR screen, and a
            // stale one left behind is what that screen would show if its own render failed.
            VaultMode::AboutQr { quantum: _ } => {
                self.qr_override = None;
                self.qr_caption = None;
                self.to_menu()
            }
            VaultMode::ShowBookmarkQr { quantum: _ } => {
                match k {
                    // browse the collection without going back to the list first
                    '↑' | '↓' => {
                        self.bookmark_cursor =
                            step_cursor(self.bookmark_cursor, self.bookmark_cache.len(), k == '↑');
                        self.request_bookmark_qr();
                    }
                    '←' => {
                        self.qr_override = None;
                        self.qr_caption = None;
                        *self.mode.lock().unwrap() = VaultMode::BookmarkList;
                        // coming back from another screen: repaint all of it, not one row
                        self.list_quantum = 0;
                        self.list_focus_ms = self.tt.elapsed_ms();
                        self.redraw();
                    }
                    _ => {}
                }
                None
            }
            VaultMode::ShowUrl => {
                // Three plain buttons rather than a radio modal stacked on top of this
                // screen: the labels already say what each does, and the modal hid them.
                match k {
                    '←' => {
                        self.show_url = None;
                        return self.to_menu();
                    }
                    '🔥' => {
                        // rescan; main.rs owns the camera because AcquireQr is blocking
                        self.show_url = None;
                        *self.mode.lock().unwrap() = VaultMode::Idle;
                        xous::send_message(
                            self.main_cid,
                            xous::Message::new_scalar(
                                VaultOp::ScanUrl.to_usize().unwrap(),
                                0,
                                0,
                                0,
                                0,
                            ),
                        )
                        .ok();
                        return None;
                    }
                    '→' => {
                        if let Some(ref url_str) = self.show_url.clone() {
                            let ipc = crate::IpcString { s: url_str.clone() };
                            // Not an expect. A failed allocation here would take down the
                            // whole UI process rather than lose one save, and the screen
                            // this button leads to can report the failure instead.
                            match xous_ipc::Buffer::into_buf(ipc) {
                                // ActionManager reports success or failure itself
                                Ok(buf) => {
                                    buf.lend(
                                        self.actions_conn,
                                        ActionOp::SaveBookmark.to_u32().unwrap(),
                                    )
                                    .ok();
                                }
                                Err(e) => log::error!("save bookmark: IPC buffer error: {:?}", e),
                            }
                        }
                        self.show_url = None;
                        *self.mode.lock().unwrap() = VaultMode::BookmarkList;
                        // coming back from another screen: repaint all of it, not one row
                        self.list_quantum = 0;
                        self.list_focus_ms = self.tt.elapsed_ms();
                        self.load_bookmarks();
                    }
                    _ => {}
                }
                self.redraw();
                None
            }
            // No catch-all. Every VaultMode is named above, so the one that used to sit here
            // could never run - and without it the compiler now refuses a new screen that
            // has no key handling, which is how About ended up with no way out of it.
        };
        self.animate.store(self.mode.lock().unwrap().should_animate(), Ordering::SeqCst);
        // don't redraw if menu is being raised
        if k != '∴' {
            self.redraw();
        }
        filtered_k
    }

    pub(crate) fn camera_transition(&mut self) {
        self.gfx.clear().ok();
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 40), Point::new(127, 120))),
        );
        tv.invert = true;
        tv.margin = Point::new(2, 2);
        tv.style = crate::theme::FONT;
        tv.draw_border = false;
        // This splash is the only place the camera controls can be shown. Once the camera
        // starts it owns the panel outright and this side is blocked inside the acquire call,
        // so there is no later opportunity to label the buttons.
        write!(tv, "starting camera\n\nright = photo\nany other = exit").ok();
        self.gfx.draw_textview(&mut tv).ok();
        self.redraw();
    }

    pub(crate) fn filter(&mut self, criteria: &String) {
        self.filter = criteria.to_owned();
        // only filter passwords in this implementation
        if self.filter.is_empty() {
            self.item_lists.lock().unwrap().filter_reset(VaultMode::Password);
        } else {
            self.item_lists.lock().unwrap().filter_reset(VaultMode::Password);
            self.item_lists.lock().unwrap().filter(VaultMode::Password, criteria);
        }
    }

    pub(crate) fn get_filter(&self) -> String { self.filter.to_owned() }

    pub(crate) fn handle_autotype(&mut self, guid: String, type_username: bool) -> Result<(), String> {
        // we re-fetch the entry for autotype, because the PDDB could have unmounted a basis.
        let atime = utc_now().timestamp() as u64;
        let pddb_binding = self.pddb.borrow();

        let mut record = pddb_binding
            .get(
                dc34_vault::VAULT_PASSWORD_DICT,
                &guid,
                None,
                false,
                false,
                None,
                Some(dc34_vault::basis_change),
            )
            .map_err(|e| format!("couldn't access key {}: {:?}", guid, e))?;
        let mut data = Vec::<u8>::new();
        record.read_to_end(&mut data).map_err(|_| format!("Couldn't access key {}", guid))?;
        let mut pw = crate::storage::PasswordRecord::try_from(data)
            .map_err(|_| format!("Couldn't deserialize {}", guid))?;
        let to_type = if type_username { &pw.username } else { &pw.password };
        self.usb_dev.send_str(to_type).ok(); // ignore USB errors
        pw.count += 1;
        pw.atime = atime;

        // this get determines which basis the key is in
        let app_data = pddb_binding
            .get(
                dc34_vault::VAULT_PASSWORD_DICT,
                &guid,
                None,
                true,
                true,
                Some(256),
                Some(dc34_vault::basis_change),
            )
            .map_err(|e| format!("error updating key atime: {:?}", e))?;
        let basis = app_data.attributes().map_err(|_| "couldn't get attributes")?.basis;

        // delete the old key
        pddb_binding
            .delete_key(dc34_vault::VAULT_PASSWORD_DICT, &guid, Some(&basis))
            .map_err(|_| "Couldn't delete previous pw entry")?;

        // write the new key in
        let mut record = pddb_binding
            .get(
                dc34_vault::VAULT_PASSWORD_DICT,
                &guid,
                Some(&basis),
                false,
                true,
                Some(dc34_vault::VAULT_ALLOC_HINT),
                Some(dc34_vault::basis_change),
            )
            .map_err(|e| format!("couldn't update key {}: {:?}", guid, e))?;
        let ser: Vec<u8> = crate::storage::PasswordRecord::into(pw);
        record.write(&ser).map_err(|e| format!("couldn't update key {}: {:?}", guid, e))?;

        self.pddb.borrow().sync().ok();
        Ok(())
    }
}

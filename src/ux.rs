use core::fmt::Write as TextViewWrite;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use bao1x_hal_service::Adc;
use blitstr2::GlyphStyle;

/// Patterns implemented in dc34-console; index 0 restores gene expression.
const LED_PATTERN_COUNT: usize = 5;
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
const FACTORY_TIMEOUT_S: u64 = 90;
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
pub const FONT_LIST: [&'static str; 6] = ["regular", "tall", "mono", "bold", "large", "small"];
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
fn style_to_name(style: &GlyphStyle) -> String {
    match style {
        GlyphStyle::Regular => "regular".to_string(),
        GlyphStyle::Monospace => "mono".to_string(),
        GlyphStyle::Cjk => "cjk".to_string(),
        GlyphStyle::Bold => "bold".to_string(),
        GlyphStyle::Large => "large".to_string(),
        GlyphStyle::Small => "small".to_string(),
        GlyphStyle::Tall => "tall".to_string(),
        _ => "regular".to_string(),
    }
}
const VAULT_CONFIG_DICT: &'static str = "vault.config";
const VAULT_CONFIG_KEY_FONT: &'static str = "fontstyle";

#[derive(PartialEq, Eq, Clone, Copy)]
enum DisplayOrientation {
    Normal,
    UpsideDown,
}

/// This test doesn't have a "scan" state because to enter it, you need to scan.
enum StandAloneTestState {
    JogPress { seen_press: bool },
    Up { seen_up: bool },
    Down { seen_down: bool },
    Left { seen_left: bool },
    Right { seen_right: bool },
    Flip { orientation_changed: bool },
    Finish { seen_button: bool },
    Exit,
    Error(String),
}

impl StandAloneTestState {
    fn handle_input(self, k: Option<char>, err: Option<String>) -> Self {
        if let Some(e) = err {
            Self::Error(e)
        } else {
            match self {
                Self::JogPress { seen_press } => {
                    let seen_press = seen_press || k.unwrap_or('\0') == '∴';
                    if seen_press { Self::Up { seen_up: false } } else { Self::JogPress { seen_press } }
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
                    let seen_left = seen_left || k.unwrap_or('\0') == '←';

                    if seen_left { Self::Right { seen_right: false } } else { Self::Left { seen_left } }
                }

                Self::Right { seen_right } => {
                    let seen_right = seen_right || k.unwrap_or('\0') == '→';

                    if seen_right {
                        Self::Flip { orientation_changed: false }
                    } else {
                        Self::Right { seen_right }
                    }
                }

                Self::Flip { orientation_changed } => {
                    let orientation_changed =
                        orientation_changed || k.unwrap_or('\0') == '🔽' || k.unwrap_or('\0') == '🔼';

                    if orientation_changed {
                        log::info!("Resetting tour state...");
                        let pddb = Pddb::new();
                        let mut key = pddb
                            .get(DC34_DICT, DC34_TOUR, None, true, true, Some(1), None::<fn()>)
                            .expect("couldn't get PDDB key");
                        key.write(&[0]).ok();
                        pddb.sync().ok();
                        log::info!("...done!");
                        Self::Finish { seen_button: false }
                    } else {
                        Self::Flip { orientation_changed }
                    }
                }

                Self::Finish { seen_button } => {
                    let seen_button = seen_button
                        || k.unwrap_or('\0') == '←'
                        || k.unwrap_or('\0') == '→'
                        || k.unwrap_or('\0') == '🔥';
                    if seen_button { Self::Exit } else { Self::Finish { seen_button } }
                }

                other => other,
            }
        }
    }

    fn is_terminal(&self) -> bool { matches!(self, StandAloneTestState::Exit) }
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

macro_rules! tour_advance {
    ($self:expr, $k:expr;
     auto { $($from:ident => $to:ident),* $(,)? }
     custom { $($pat:pat => $body:expr),* $(,)? }
    ) => {
        match $self {
            $(
                Self::$from { seen_press } => {
                    if seen_press || is_tour_advance_key($k) {
                        Self::$to { seen_press: false }
                    } else {
                        Self::$from { seen_press }
                    }
                }
            )*
            $(
                $pat => $body,
            )*
        }
    }
}

fn is_tour_advance_key(k: char) -> bool {
    // up/down not used to advance because it's too easy to fat-finger with menu raising
    k == '←' || k == '→' || k == '🔥' // || k == '↑' || k == '↓'
}

enum TourState {
    Welcome { seen_press: bool },
    LightGeneExplainer1 { seen_press: bool },
    LightGeneExplainer2 { seen_press: bool },
    Breeding1 { seen_press: bool },
    Breeding2 { seen_press: bool },
    Breeding3 { seen_press: bool },
    Breeding4 { seen_press: bool },
    BadgeRecap { seen_press: bool },
    TokenIntro1 { seen_press: bool },
    TokenIntro2 { seen_press: bool },
    TokenIntro3 { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl TourState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                LightGeneExplainer1 => LightGeneExplainer2,
                LightGeneExplainer2 => Breeding1,
                Breeding1          => Breeding2,
                Breeding2          => Breeding3,
                Breeding3          => Breeding4,
                Breeding4          => BadgeRecap,
                BadgeRecap         => TokenIntro1,
                TokenIntro1        => TokenIntro2,
                TokenIntro2        => TokenIntro3,
                TokenIntro3        => InfoScreen,
                InfoScreen         => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Welcome { seen_press } => {
                    // only advance if the jog dial press in is discovered: the point
                    // of this screen is to make sure users are aware of this interaction
                    // pattern.
                    let seen_press = seen_press || k == '∴';
                    // log::info!("k: {}, seen_press: {:?}", k, seen_press);
                    if seen_press {
                        Self::LightGeneExplainer1 { seen_press: false }
                    } else {
                        Self::Welcome { seen_press }
                    }
                },
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool { matches!(self, TourState::End { seen_press: _ } | TourState::Error(_)) }
}

enum HelpState {
    BadgeRecap { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl HelpState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                BadgeRecap         => InfoScreen,
                InfoScreen         => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool { matches!(self, HelpState::End { seen_press: _ } | HelpState::Error(_)) }
}

enum TokenHelpState {
    TokenRecap { seen_press: bool },
    Extension { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl TokenHelpState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                TokenRecap         => Extension,
                Extension          => InfoScreen,
                InfoScreen         => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool {
        matches!(self, TokenHelpState::End { seen_press: _ } | TokenHelpState::Error(_))
    }
}
enum TokenTourState {
    TokenTour1 { seen_press: bool },
    TokenTour2 { seen_press: bool },
    TokenTour3 { seen_press: bool },
    TokenRecap { seen_press: bool },
    BrowserExtension { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl TokenTourState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                TokenTour1         => TokenTour2,
                TokenTour2         => TokenTour3,
                TokenTour3         => TokenRecap,
                TokenRecap         => BrowserExtension,
                BrowserExtension   => InfoScreen,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::InfoScreen { seen_press } => {
                    if seen_press || is_tour_advance_key(k) {
                        crate::config::side_effect_skip_token_tour(true);
                        Self::End { seen_press: false }
                    } else {
                        Self::InfoScreen { seen_press }
                    }
                },
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool {
        matches!(self, TokenTourState::End { seen_press: _ } | TokenTourState::Error(_))
    }
}

enum AboutState {
    BaochipLogo { seen_press: bool },
    Bunnie { seen_press: bool },
    Cheeso { seen_press: bool },
    InfoScreen { seen_press: bool },
    Diagnostics { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl AboutState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                Bunnie             => BaochipLogo,
                BaochipLogo        => Cheeso,
                Cheeso             => InfoScreen,
                InfoScreen         => Diagnostics,
                Diagnostics        => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool { matches!(self, AboutState::End { seen_press: _ } | AboutState::Error(_)) }

    fn is_diagnostics(&self) -> bool { matches!(self, AboutState::Diagnostics { seen_press: _ }) }
}

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

    // various state machines
    start_time: Option<Instant>,
    factory_test: FactoryTestState,
    tour_state: TourState,
    token_tour_state: TokenTourState,
    token_help_state: TokenHelpState,
    help_state: HelpState,
    about_state: AboutState,
    standalone_test: StandAloneTestState,

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

    // adc for reading battery level
    adc: Adc,
    batt_polled: bool,
    low_batt_since: Option<Instant>,

    // modals for ShowUrl confirmation (type-out flow)
    modals: modals::Modals,
    pub user_bitmap: Option<[u32; 512]>,
    phase: bool,
    edge: bool,
    last_mode: VaultMode,
    pub bio_loaded: bool,
}

/// Standby images that ship with the firmware. Captured photos are appended to this list
/// at runtime, so "set a photo as the standby image" needs no separate mechanism.
/// Return the slice of `text` to show this tick, scrolling if it does not fit.
///
/// The panel fits 18 monospace cells. Anything longer is scrolled one character every four
/// ticks, with a gap so the end and the beginning are distinguishable when it wraps. Short
/// text is returned unchanged rather than scrolled pointlessly.
fn marquee(text: &str, quantum: u32) -> String {
    const VISIBLE: usize = 18;
    let count = text.chars().count();
    if count <= VISIBLE {
        return text.to_string();
    }
    let padded: Vec<char> = text.chars().chain("   ".chars()).collect();
    let offset = (quantum as usize / 2) % padded.len();
    padded.iter().cycle().skip(offset).take(VISIBLE).collect()
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

pub const BUILTIN_IMAGES: [&str; 2] = ["S-CAM", "DEFCON"];

/// Index into BUILTIN_IMAGES for the DEFCON logo. Named because the idle draw has to tell
/// it from the S-CAM logo, and both are "no user bitmap".
pub const DEFCON_IMAGE: usize = 1;

/// Blinky choices. Index 0 is gene expression - the badge's protected behaviour and the
/// default - and the rest map onto dc34-console's pattern table.
pub const BLINKY_CHOICES: [&str; 6] =
    ["GENE (DEFAULT)", "RAINBOW", "CHASE", "BREATHE", "EMBER", "RIOT"];

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
            start_time: None,
            factory_test: FactoryTestState::InitWait { start_time: std::time::Instant::now() },
            tour_state: TourState::Welcome { seen_press: false },
            token_tour_state: TokenTourState::TokenTour1 { seen_press: false },
            help_state: HelpState::BadgeRecap { seen_press: false },
            about_state: AboutState::Bunnie { seen_press: false },
            standalone_test: StandAloneTestState::JogPress { seen_press: false },
            token_help_state: TokenHelpState::TokenRecap { seen_press: false },
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
            standby_drawn: None,
            blinky_cursor: 0,
            show_url: None,
            bookmark_cache: Vec::new(),
            bookmark_cursor: 0,
            modals: modals::Modals::new(xns).unwrap(),
            adc: Adc::new(),
            batt_polled: false,
            low_batt_since: None,
            user_bitmap: None,
            phase: false,
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
                        // Truncate URL for display to fit the small-font 21-column screen
                        let display = if url.len() > 30 {
                            format!("{}\u{2026}", &url[..29])
                        } else {
                            url
                        };
                        entries.push((key.clone(), display, label));
                    }
                }
            }
        }
        // Sort by key (zero-padded hex u64) so entries appear in insertion order
        entries.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
        self.bookmark_cache = entries;
    }

    pub fn reset_help_state(&mut self) { self.help_state = HelpState::BadgeRecap { seen_press: false }; }

    pub fn reset_about_state(&mut self) { self.about_state = AboutState::Bunnie { seen_press: false }; }

    pub fn reset_token_help_state(&mut self) {
        self.token_help_state = TokenHelpState::TokenRecap { seen_press: false };
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

    pub(crate) fn store_glyph_style(&mut self, style: GlyphStyle) {
        self.pddb
            .borrow()
            .delete_key(VAULT_CONFIG_DICT, VAULT_CONFIG_KEY_FONT, Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS))
            .ok();

        match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            true,
            true,
            Some(32),
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut style_key) => {
                style_key.write(style_to_name(&style).as_bytes()).ok();
            }
            _ => panic!("PDDB access erorr"),
        };
        self.pddb.borrow().sync().ok();
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
                    &rows, self.passkey_cursor, "NO PASSKEYS STORED",
                    crate::theme::ListStyle::Ghost,
                );
                let has = !rows.is_empty();
                crate::theme::button_labels(
                    &self.gfx, self.screen_size,
                    Some("back"),
                    if has { Some("del") } else { None },
                    None,
                );
                self.gfx.flush().ok();
            }
            VaultMode::PhotoList => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "PHOTOS");
                crate::theme::list(
                    &self.gfx, self.screen_size, self.item_height,
                    &self.photo_cache, self.photo_cursor, "NO PHOTOS YET",
                    crate::theme::ListStyle::Numbered,
                );
                let has = !self.photo_cache.is_empty();
                crate::theme::button_labels(
                    &self.gfx, self.screen_size,
                    Some("back"),
                    if has { Some("del") } else { None },
                    if has { Some("view") } else { None },
                );
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
                        &self.gfx, self.screen_size, Some("back"), None, None,
                    );
                }
                self.gfx.flush().ok();
            }
            VaultMode::SettingsBling => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "BLING");
                let mut rows: Vec<String> =
                    BUILTIN_IMAGES.iter().map(|s| s.to_string()).collect();
                rows.extend(self.photo_cache.iter().cloned());
                crate::theme::list(
                    &self.gfx, self.screen_size, self.item_height,
                    &rows, self.bling_cursor, "NO IMAGES",
                    crate::theme::ListStyle::Select { marked: Some(self.standby_choice) },
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
                    &rows, self.blinky_cursor, "NO PATTERNS",
                    crate::theme::ListStyle::Select { marked: Some(self.led_pattern) },
                );
                // patterns need the carrier; say so rather than offering a dead control
                crate::theme::button_labels(
                    &self.gfx, self.screen_size, Some("back"), None, Some("pick"),
                );
                self.gfx.flush().ok();
            }

            VaultMode::AboutQr { quantum: _ } => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "ABOUT");
                crate::theme::button_labels(&self.gfx, self.screen_size, Some("back"), None, None);
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
            VaultMode::ShowBookmarkQr { quantum } => {
                if let Some(code) = &self.qr_override {
                    if quantum & 7 == 0 {
                        self.clear_area();
                        let width = code.width();
                        let modules: Vec<bool> =
                            code.to_colors().into_iter().map(|c| c != Color::Light).collect();
                        self.gfx.render_qr(&modules, width, Point::new(0, 0)).ok();
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
                            write!(tv, "{}", marquee(&url, quantum)).ok();
                            self.gfx.draw_textview(&mut tv).ok();
                        }
                    }
                    // This arm only runs while the mode IS ShowBookmarkQr, so the two Idle
                    // branches that used to sit here could never fire. Just advance the tick
                    // that drives the redraw cadence and the caption scroll.
                    *self.mode.lock().unwrap() = VaultMode::ShowBookmarkQr { quantum: quantum + 1 };
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
                    &self.gfx, self.screen_size, Some("back"), None, Some("send"),
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
                    // The empty state used to return here, before any labels were drawn, which
                    // left the screen with no way out and no indication there was one.
                    crate::theme::heading(&self.gfx, self.screen_size, "PASSWORDS");
                    crate::theme::button_labels(
                        &self.gfx, self.screen_size, Some("back"), None, None,
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
                    &self.gfx, self.screen_size, Some("back"), None, Some("type"),
                );
                self.gfx.flush().ok();
            }// _ => unimplemented!(),
            VaultMode::BookmarkList => {
                self.clear_area();
                crate::theme::heading(&self.gfx, self.screen_size, "QR COLLECTION");
                let rows: Vec<String> = self
                    .bookmark_cache
                    .iter()
                    .map(|(_, display, label)| {
                        if label.is_empty() {
                            display.clone()
                        } else {
                            format!("{}: {}", label, display)
                        }
                    })
                    .collect();
                crate::theme::list(
                    &self.gfx, self.screen_size, self.item_height,
                    &rows, self.bookmark_cursor, "NO BOOKMARKS YET",
                    crate::theme::ListStyle::Numbered,
                );
                let has = !rows.is_empty();
                crate::theme::button_labels(
                    &self.gfx, self.screen_size,
                    Some("back"), None,
                    if has { Some("show") } else { None },
                );
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
                    Some("back"), Some("retry"), Some("save"),
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

    /// Load the photo under the photos-list cursor for full-screen viewing.
    fn show_selected_photo(&mut self) {
        if let Some(key) = self.photo_cache.get(self.photo_cursor).cloned() {
            match crate::storage::photo_get(&self.pddb.borrow(), &key) {
                Some(bits) => {
                    self.pending_photo = Some(bits);
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
                    '→' => {
                        if let Some(item) = self.get_selected_item() {
                            // print any errors within this function as a panic at this line
                            self.handle_autotype(item.guid, false).unwrap();
                        }
                    }
                    '\u{0}' => {
                        {
                            *self.mode.lock().unwrap() = VaultMode::Totp;
                        }
                        // reload DB on mode switch
                        xous::send_message(
                            self.actions_conn,
                            xous::Message::new_blocking_scalar(
                                ActionOp::ReloadDb.to_usize().unwrap(),
                                0,
                                0,
                                0,
                                0,
                            ),
                        )
                        .ok();
                        self.refresh_draw_list();
                    }
                    _ => {
                        // log::warn!("Password unhandled char: {}", k)
                    }
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
                    '→' => {
                        if let Some(code) = self.update_selected_totp_code() {
                            // ignore USB errors while sending code
                            self.usb_dev.send_str(&code).ok();
                        }
                    }
                    '\u{0}' => {
                        {
                            // lock needs to go out of scope so we don't hang the later ops
                            *self.mode.lock().unwrap() = VaultMode::Password;
                        }
                        // reload DB on mode switch
                        xous::send_message(
                            self.actions_conn,
                            xous::Message::new_blocking_scalar(
                                ActionOp::ReloadDb.to_usize().unwrap(),
                                0,
                                0,
                                0,
                                0,
                            ),
                        )
                        .ok();
                        self.refresh_draw_list();
                    }
                    _ => {
                        // log::warn!("TOTP unhandled char: {}", k)
                    }
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
                    '🔥' => {
                        if let Some(p) = self.passkey_cache.get(self.passkey_cursor) {
                            let key = p.key.clone();
                            if let Err(e) =
                                crate::storage::passkey_delete(&self.pddb.borrow(), &key)
                            {
                                log::warn!("could not delete passkey {}: {:?}", key, e);
                            }
                            self.load_passkeys();
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
                    '→' => {
                        self.show_selected_photo();
                        return None;
                    }
                    '🔥' => {
                        if let Some(key) = self.photo_cache.get(self.photo_cursor).cloned() {
                            if let Err(e) =
                                crate::storage::photo_delete(&self.pddb.borrow(), &key)
                            {
                                log::warn!("could not delete photo {}: {:?}", key, e);
                            }
                            self.load_photos();
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
                        self.modals.show_notification("PHOTO STORE FULL", None).ok();
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
                        *self.mode.lock().unwrap() = VaultMode::PhotoList;
                    }
                    // browse without going back to the list
                    '↑' | '↓' => {
                        self.photo_cursor =
                            step_cursor(self.photo_cursor, self.photo_cache.len(), k == '↑');
                        self.show_selected_photo();
                    }
                    _ => {}
                }
                self.redraw();
                None
            }
            VaultMode::SettingsBling => {
                let mut leaving = false;
                let count = BUILTIN_IMAGES.len() + self.photo_cache.len();
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
                '←' => {
                    if self.qr_override.is_some() {
                        self.qr_override = None;
                    } else {
                        let url = crate::storage::default_bookmark_url(&self.pddb.borrow());
                        match url {
                            Some(u) => match QrCode::with_error_correction_level(
                                u.as_bytes(),
                                qrcode::EcLevel::M,
                            ) {
                                Ok(code) => self.qr_override = Some(code),
                                Err(e) => log::warn!("default bookmark will not fit in a QR: {:?}", e),
                            },
                            None => log::info!("no default bookmark set"),
                        }
                    }
                    self.redraw();
                    None
                }
                // MIDDLE - open the camera. Routed through the main loop because
                // ActionOp::AcquireQr is a blocking scalar and the key path cannot send one.
                '🔥' => Some(k),
                // RIGHT - context dependent. The LED ring is on the badge carrier, so
                // cycling patterns is meaningless while detached; cycle the standby image
                // instead rather than leaving a dead control.
                '→' => {
                    let attached = self
                        .global_config
                        .as_ref()
                        .map(|c| c.lock().unwrap().is_badge_attached())
                        .unwrap_or(false);
                    if attached {
                        self.led_pattern = (self.led_pattern + 1) % (LED_PATTERN_COUNT + 1);
                        if let Some(config) = self.global_config.as_ref() {
                            config.lock().unwrap().set_led_pattern(self.led_pattern);
                        }
                    } else {
                        log::info!("detached: right button cycles the standby image");
                    }
                    None
                }
                _ => Some(k),
            },

            VaultMode::BookmarkList => {
                match k {
                    '↑' | '↓' => {
                        self.bookmark_cursor =
                            step_cursor(self.bookmark_cursor, self.bookmark_cache.len(), k == '↑')
                    }
                    '←' => {
                        // back to the menu this was opened from, not past it to standby
                        return self.to_menu();
                    }
                    '→' | '🔥' => {
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
            VaultMode::AboutQr { quantum: _ } => self.to_menu(),
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
                            let buf = xous_ipc::Buffer::into_buf(ipc).expect("IpcString buf");
                            // ActionManager reports success or failure itself
                            buf.lend(self.actions_conn, ActionOp::SaveBookmark.to_u32().unwrap())
                                .ok();
                        }
                        self.show_url = None;
                        *self.mode.lock().unwrap() = VaultMode::BookmarkList;
                        self.load_bookmarks();
                    }
                    _ => {}
                }
                self.redraw();
                None
            }
            // catch-all for now
            _ => Some(k),
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
        write!(tv, "STARTING CAMERA\n\nRIGHT = PHOTO\nANY OTHER = EXIT").ok();
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

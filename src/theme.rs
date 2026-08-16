//! S-CAM visual theme: one place for the badge name, the font, and the screen furniture.
//!
//! Kept deliberately small. The badge renders text by sending `TextView`s to the graphics
//! server, so a "theme" here is a handful of constants plus two helpers that every screen
//! draws through — a heading and the contextual button-label bar.
//!
//! Strings are plain Rust constants rather than `t!()` lookups. See DEAD-PATHS.md: this
//! crate's `locales/i18n.json` is never read by the build, and routing new strings through
//! the translation system would force every UI change into the shared xous-core image.

use blitstr2::GlyphStyle;
use ux_api::minigfx::*;
use ux_api::service::api::*;

/// The badge's name. Change here and it changes everywhere on screen.
pub const BADGE_NAME: &str = "S-CAM";

/// URL the About screen renders as a QR code. Must stay within CAP_QR_DISPLAY (106 bytes)
/// — the badge renders this for a phone to scan, which is the tighter of the two QR limits.
pub const ABOUT_URL: &str = "https://github.com/heckseven/s-cam";

/// Departure Mono at 11px. Lives in the flash-resident graphics server, so it costs the
/// app's page budget nothing.
pub const FONT: GlyphStyle = GlyphStyle::DepartureMono;

/// Height of the button-label bar, in pixels. Matches the 14px Departure Mono cell.
pub const LABEL_BAR_H: isize = 14;

/// Draw an ALL-CAPS heading across the top of the screen.
///
/// Takes the title already uppercased by the caller where it is a literal; `to_uppercase`
/// here would allocate on every redraw for no benefit.
pub fn heading(gfx: &ux_api::service::gfx::Gfx, screen: Point, title: &str) {
    let mut tv = TextView::new(
        Gid::dummy(),
        TextBounds::BoundingBox(Rectangle::new(
            Point::new(0, 0),
            Point::new(screen.x, LABEL_BAR_H),
        )),
    );
    tv.style = FONT;
    tv.draw_border = false;
    tv.invert = true;
    tv.margin = Point::new(1, 1);
    use core::fmt::Write;
    write!(tv, "{}", title).ok();
    gfx.draw_textview(&mut tv).ok();
}

/// Draw the three-slot contextual button bar along the bottom.
///
/// Slot order matches the hardware: left is tertiary, middle secondary, right primary.
/// `None` leaves a slot blank, which is how a button says "I do nothing here" rather than
/// silently being dead — the case that matters when the module is detached from the badge
/// carrier and the LED patterns have no hardware to drive.
pub fn button_labels(
    gfx: &ux_api::service::gfx::Gfx,
    screen: Point,
    left: Option<&str>,
    middle: Option<&str>,
    right: Option<&str>,
) {
    use core::fmt::Write;
    // One TextView per slot, not one row of padded columns.
    //
    // A single "{:<6}{:^6}{:>6}" row is 18 monospace cells = 126px on a 128px panel. That
    // fits on paper, but with only 2px of slack the typesetter wrapped the trailing column
    // onto a second line, and this bar is one line tall - so the RIGHT label silently
    // vanished on most screens. Independent boxes cannot push each other off the edge, and
    // a label that is too wide for its own slot now truncates visibly instead.
    let third = screen.x / 3;
    for (slot, label) in [left, middle, right].iter().enumerate() {
        let Some(text) = label else { continue };
        let x0 = slot as isize * third;
        // the last slot absorbs the rounding remainder rather than clipping
        let x1 = if slot == 2 { screen.x } else { x0 + third };
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::CenteredTop(Rectangle::new(
                Point::new(x0, screen.y - LABEL_BAR_H),
                Point::new(x1, screen.y),
            )),
        );
        tv.style = FONT;
        tv.draw_border = false;
        tv.invert = true;
        tv.margin = Point::new(0, 1);
        tv.ellipsis = true;
        write!(tv, "{}", text).ok();
        gfx.draw_textview(&mut tv).ok();
    }
}

/// How a list presents its rows.
///
/// The four S-CAM list screens differ only in what the left gutter carries, so the shape is
/// shared and the gutter is the variable.
pub enum ListStyle {
    /// No gutter. For lists where a row is just a name — passkeys, bookmarks.
    Ghost,
    /// Rows numbered from 1. For lists where position is how you refer to an item.
    Numbered,
    /// Rows carrying a persistent choice; the applied one gets a check mark.
    ///
    /// `marked` is the *applied* index, which is not the cursor: you scroll past options
    /// without choosing them, and the screen has to keep showing which one is in effect.
    Select { marked: Option<usize> },
}

/// Width of the left gutter, in pixels.
///
/// Every row reserves it, marked or not, so the text never shifts sideways when the mark
/// moves — the row is marked in place rather than indented.
const GUTTER_W: isize = 10;

/// Draw a check mark inside `gutter`, as two strokes.
///
/// Departure Mono has no U+2713 — the OTF renders it as .notdef, so writing '✓' would put a
/// missing-glyph box next to the applied item. Drawing it costs no font table and cannot
/// regress if the font is regenerated with different coverage.
fn check_mark(gfx: &ux_api::service::gfx::Gfx, gutter: Rectangle) {
    let style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
    let x = gutter.tl().x + 2;
    // +2: the row box is taller than the glyphs and its centre sits above the text's, so
    // centring on the box alone left the mark visibly high against the label.
    let y = gutter.tl().y + gutter.height() as isize / 2 + 2;
    let mut ol = ObjectList::new();
    // short down-stroke into the elbow, then the long up-stroke
    ol.push(ClipObjectType::Line(Line::new_with_style(
        Point::new(x, y),
        Point::new(x + 2, y + 2),
        style,
    )))
    .unwrap();
    ol.push(ClipObjectType::Line(Line::new_with_style(
        Point::new(x + 2, y + 2),
        Point::new(x + 6, y - 3),
        style,
    )))
    .unwrap();
    gfx.draw_object_list(ol).unwrap();
}

/// Fit a focus box around `inner`, clamped to `limit`.
///
/// Air on the right and below only. Padding all four sides made the box a pixel too wide and
/// a pixel too tall, and sat it a pixel high of the text it was marking.
fn pad(inner: Rectangle, limit: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(inner.tl().x.max(limit.tl().x), inner.tl().y.max(limit.tl().y)),
        Point::new((inner.br().x + 1).min(limit.br().x), (inner.br().y + 1).min(limit.br().y)),
    )
}

/// Cut `text` to `cols` monospace cells, ending with an ellipsis when it does not fit.
///
/// The typesetter word-wraps, and a URL is one long unbreakable word: it cannot fit on the
/// line, so it moves to a second line that a one-row box clips, leaving the row showing its
/// number and nothing else. Cutting to the column count here means there is never a wrap to
/// clip. Counts chars, not bytes, so a multi-byte character cannot split.
fn fit(text: &str, cols: usize) -> String {
    if text.chars().count() <= cols {
        return text.to_string();
    }
    let mut out: String = text.chars().take(cols.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// Width of one character cell, measured once from the graphics server.
///
/// The font's own tables say every glyph is 7px, and dividing the panel by 7 said 18
/// characters fit. The server disagreed: it truncated well before that, which is where the
/// ellipsis nobody asked for kept coming from, and switching the mark off just turned the
/// overrun into a dropped row. The tables are not the authority on what fits - the server
/// that lays the text out is - so this asks it and caches the answer.
fn cell_width(gfx: &ux_api::service::gfx::Gfx) -> isize {
    use core::sync::atomic::{AtomicUsize, Ordering};
    use core::fmt::Write;
    static MEASURED: AtomicUsize = AtomicUsize::new(0);

    let cached = MEASURED.load(Ordering::Relaxed);
    if cached != 0 {
        return cached as isize;
    }
    const PROBE: isize = 8;
    let mut tv = TextView::new(Gid::dummy(), TextBounds::GrowableFromTl(Point::new(0, 0), 512));
    tv.style = FONT;
    tv.draw_border = false;
    tv.margin = Point::new(0, 0);
    let _ = write!(tv, "MMMMMMMM");
    let measured = if gfx.bounds_compute_textview(&mut tv).is_ok() {
        tv.bounds_computed.map(|b| b.br.x - b.tl.x).unwrap_or(0)
    } else {
        0
    };
    // Round up: a cell that is really 7.5px wide has to be budgeted as 8, or the last
    // character on every row falls off the end.
    let cell = if measured > 0 { (measured + PROBE - 1) / PROBE } else { 7 };
    log::info!("list: measured cell width {}px ({}px for {} chars)", cell, measured, PROBE);
    MEASURED.store(cell as usize, Ordering::Relaxed);
    cell
}

/// Take at most `cols` characters, without marking the cut.
///
/// A scrolling list does not want an ellipsis: focus the row and the rest of the text
/// arrives on its own, so the mark would promise nothing new while eating a column that
/// could have shown another character.
fn clip(text: &str, cols: usize) -> String { text.chars().take(cols).collect() }

/// Which rows a `list()` call should paint.
///
/// A marquee advances several times a second. Repainting every row, the heading and the
/// button bar at that rate visibly flashes - the same fault that made the photo grid
/// unusable - so an animation tick repaints only the row that is actually moving.
#[derive(Clone, Copy, PartialEq)]
pub enum Repaint {
    All,
    FocusedRow,
}

/// How long a row stays still after it gains focus, before it starts scrolling.
pub const MARQUEE_HOLD_MS: u64 = 1000;

/// How long each character step takes once it is scrolling.
const MARQUEE_STEP_MS: u64 = 250;

/// Return the slice of `text` to show right now, scrolling if it does not fit.
///
/// Driven by how long the row has actually held focus, not by a count of redraws. A redraw
/// count made "a second" mean "however long four repaints happen to take", which drifts with
/// whatever else the loop is doing; measuring the wall clock makes the hold exactly the hold.
///
/// Text longer than `visible` cells scrolls a character at a time, with a gap so the end and
/// the beginning stay distinguishable when it wraps. Text that already fits is returned
/// unchanged rather than scrolled pointlessly.
pub fn marquee(text: &str, held_ms: u64, visible: usize, hold_ms: u64) -> String {
    let count = text.chars().count();
    if count <= visible {
        return text.to_string();
    }
    if held_ms < hold_ms {
        return text.chars().take(visible).collect();
    }
    let padded: Vec<char> = text.chars().chain("   ".chars()).collect();
    let offset = ((held_ms - hold_ms) / MARQUEE_STEP_MS) as usize % padded.len();
    padded.iter().cycle().skip(offset).take(visible).collect()
}

/// Draw a scrolling list with a cursor, between the heading and the button bar.
///
/// Every S-CAM list screen — passkeys, photos, images, patterns — is this same shape, so
/// they share one implementation rather than four near-copies that drift apart.
///
/// `cursor` is an index into `items`; the view scrolls to keep it visible. An empty list
/// draws `empty_msg` instead, because a blank screen is indistinguishable from a hang.
pub fn list(
    gfx: &ux_api::service::gfx::Gfx,
    screen: Point,
    row_h: isize,
    items: &[String],
    cursor: usize,
    empty_msg: &str,
    style: ListStyle,
    scroll: Option<u64>,
    repaint: Repaint,
) {
    use core::fmt::Write;
    let top = LABEL_BAR_H;
    let bottom = screen.y - LABEL_BAR_H;
    let rows = ((bottom - top) / row_h).max(1) as usize;

    if items.is_empty() {
        if repaint == Repaint::FocusedRow {
            return;
        }
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::BoundingBox(Rectangle::new(
                Point::new(0, top),
                Point::new(screen.x, top + row_h),
            )),
        );
        tv.style = FONT;
        tv.draw_border = false;
        tv.invert = true;
        write!(tv, "{}", empty_msg).ok();
        gfx.draw_textview(&mut tv).ok();
        return;
    }

    // Only a check mark needs a gutter. A numbered row carries its number in the text, so
    // giving it a gutter too indented it twice and pushed the first character out of line with
    // the heading above.
    let gutter = match style {
        ListStyle::Ghost | ListStyle::Numbered => 0,
        ListStyle::Select { .. } => GUTTER_W,
    };

    // scroll so the cursor stays on screen
    let first = if cursor >= rows { cursor + 1 - rows } else { 0 };
    for (n, item) in items.iter().skip(first).take(rows).enumerate() {
        let index = first + n;
        if repaint == Repaint::FocusedRow && index != cursor {
            continue;
        }
        let y = top + (n as isize) * row_h;
        let row = Rectangle::new(Point::new(0, y), Point::new(screen.x, y + row_h));

        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::BoundingBox(Rectangle::new(
                Point::new(gutter, y),
                Point::new(screen.x, y + row_h),
            )),
        );
        tv.style = FONT;
        tv.draw_border = false;
        // same left margin as heading(), so a row's first character sits directly under the
        // heading's first character
        tv.margin = Point::new(1, 0);
        // Left on everywhere, including scrolling lists that do not want to show one. With
        // the width measured rather than guessed the string should always fit and the mark
        // should never appear - but if the measurement is ever wrong, a marked row is a row
        // you can still read, and a row that overruns with the mark off is dropped to its
        // number and nothing else. Degrade to the ugly option, not the useless one.
        tv.ellipsis = true;
        // Every row is white-on-black. Focus is the brackets, not an inverted slab: the
        // inverted row was the only black-on-white text on the panel and read as a blank bar.
        tv.invert = true;
        // Hold back one cell: the server insets by its own margins as well as ours, and one
        // spare column costs a character while an overrun costs the whole row.
        let usable = screen.x - gutter - tv.margin.x * 2;
        let cols = ((usable / cell_width(gfx)).max(1) as usize).saturating_sub(1).max(1);
        match style {
            ListStyle::Numbered => {
                let prefix = format!("{}. ", index + 1);
                let room = cols.saturating_sub(prefix.chars().count());
                // The focused row scrolls its full text rather than losing the tail to an
                // ellipsis - on a saved URL the distinguishing part is usually the tail.
                let body = match scroll {
                    // The focused row shows what fits, then scrolls the whole thing.
                    Some(held) if index == cursor => marquee(item, held, room, MARQUEE_HOLD_MS),
                    // Other rows in a scrolling list are cut without an ellipsis - the
                    // text is reachable by focusing the row, so the mark buys nothing.
                    Some(_) => clip(item, room),
                    None => fit(item, room),
                };
                write!(tv, "{}{}", prefix, body).ok()
            }
            _ => write!(tv, "{}", fit(item, cols)).ok(),
        };
        gfx.draw_textview(&mut tv).ok();
        if let ListStyle::Select { marked: Some(m) } = style {
            if m == index {
                check_mark(gfx, Rectangle::new(Point::new(0, y), Point::new(gutter, y + row_h)));
            }
        }
        if index == cursor {
            // Wrap the text, not the row. The server reports what it actually laid out, so
            // the brackets track the label's width instead of spanning the full 128px and
            // reading as a box around empty space.
            let around = tv.bounds_computed.map(|b| pad(b, row)).unwrap_or(row);
            ux_api::widgets::scroll::draw_corner_brackets(gfx, around);
        }
    }
}

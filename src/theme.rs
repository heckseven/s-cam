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
) {
    use core::fmt::Write;
    let top = LABEL_BAR_H;
    let bottom = screen.y - LABEL_BAR_H;
    let rows = ((bottom - top) / row_h).max(1) as usize;

    if items.is_empty() {
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
        // Truncate an over-long row rather than dropping it. Without this the typesetter
        // aborts on overflow and the row renders as its number and nothing else.
        tv.ellipsis = true;
        // Every row is white-on-black. Focus is the brackets, not an inverted slab: the
        // inverted row was the only black-on-white text on the panel and read as a blank bar.
        tv.invert = true;
        match style {
            ListStyle::Numbered => write!(tv, "{}. {}", index + 1, item).ok(),
            _ => write!(tv, "{}", item).ok(),
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

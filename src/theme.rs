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
    let mut tv = TextView::new(
        Gid::dummy(),
        TextBounds::BoundingBox(Rectangle::new(
            Point::new(0, screen.y - LABEL_BAR_H),
            Point::new(screen.x, screen.y),
        )),
    );
    tv.style = FONT;
    tv.draw_border = false;
    tv.invert = true;
    tv.margin = Point::new(1, 1);
    use core::fmt::Write;
    // three fixed columns so a label never appears under the wrong button
    write!(
        tv,
        "{:<6}{:^6}{:>6}",
        left.unwrap_or(""),
        middle.unwrap_or(""),
        right.unwrap_or("")
    )
    .ok();
    gfx.draw_textview(&mut tv).ok();
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
        write!(tv, "{}", empty_msg).ok();
        gfx.draw_textview(&mut tv).ok();
        return;
    }

    // scroll so the cursor stays on screen
    let first = if cursor >= rows { cursor + 1 - rows } else { 0 };
    for (n, item) in items.iter().skip(first).take(rows).enumerate() {
        let y = top + (n as isize) * row_h;
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::BoundingBox(Rectangle::new(
                Point::new(0, y),
                Point::new(screen.x, y + row_h),
            )),
        );
        tv.style = FONT;
        tv.draw_border = false;
        // the selected row is inverted rather than marked with a glyph, so the cursor is
        // legible at a glance on a 128px panel
        tv.invert = first + n == cursor;
        write!(tv, "{}", item).ok();
        gfx.draw_textview(&mut tv).ok();
    }
}

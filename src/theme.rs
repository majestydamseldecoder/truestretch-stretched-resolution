//! Neutral desktop palette with one accent. Every constant mirrors a CSS token in
//! `docs/design/mockup.html`.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

/// Status bar and input fields.
pub const TITLE: Color32 = hex(0x1E1F22);
/// Sidebar and cards.
pub const SIDE: Color32 = hex(0x2B2D31);
/// Content area.
pub const MAIN: Color32 = hex(0x313338);
/// Hovered row.
pub const HOVER: Color32 = hex(0x393C41);
/// Selected row.
pub const SELECTED: Color32 = hex(0x404249);
/// Hairlines and separators.
pub const LINE: Color32 = hex(0x3F4147);
/// Headings and selected text.
pub const TEXT: Color32 = hex(0xF2F3F5);
/// Body text.
pub const BODY: Color32 = hex(0xDBDEE1);
/// Secondary text, row subtitles.
pub const FAINT: Color32 = hex(0x949BA4);
/// Disabled text, captions.
pub const MUTE: Color32 = hex(0x6D6F78);
/// The single accent: primary buttons, active tab, switches.
pub const ACCENT: Color32 = hex(0xF5A524);
/// Success state (game running, mode restored).
pub const OK: Color32 = hex(0x23A55A);
/// Error state.
pub const ERR: Color32 = hex(0xF23F43);

/// SMPTE 75% color bars, used only for the app icon.
pub const BARS: [Color32; 7] =
    [hex(0xC0C0C0), hex(0xC0C000), hex(0x00C0C0), hex(0x00C000), hex(0xC000C0), hex(0xC00000), hex(0x0000C0)];

/// Corner radius used for cards, rows and buttons.
pub const RADIUS: CornerRadius = CornerRadius::same(5);

/// `0xRRGGBB` to [`Color32`], so the constants read like the CSS tokens.
pub const fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Installs the dark visuals on the context.
pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    (v.panel_fill, v.window_fill, v.extreme_bg_color, v.faint_bg_color) = (MAIN, MAIN, TITLE, SIDE);
    v.override_text_color = Some(BODY);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.window_stroke = Stroke::new(1.0, TITLE);
    let ws = &mut v.widgets;
    for w in [&mut ws.noninteractive, &mut ws.inactive, &mut ws.hovered, &mut ws.active, &mut ws.open] {
        w.corner_radius = RADIUS;
        w.bg_stroke = Stroke::NONE;
    }
    (ws.inactive.weak_bg_fill, ws.inactive.bg_fill) = (hex(0x4E5058), TITLE);
    (ws.hovered.weak_bg_fill, ws.hovered.bg_fill, ws.active.weak_bg_fill) = (hex(0x5C5E66), HOVER, SELECTED);
    ws.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.5));
    });
}

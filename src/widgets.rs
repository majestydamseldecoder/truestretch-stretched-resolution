//! Custom-painted widgets from the design: sidebar rows, game tiles, settings rows with a
//! switch, the app icon and the split before/after stretch preview.

use crate::fonts::DISPLAY;
use crate::games::Game;
use crate::settings::Resolution;
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke};
use eframe::egui::{Vec2, pos2, vec2};

/// Font in the wide display family (used sparingly: preview tags).
pub fn display_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Monospace text for modes, paths and diff lines.
pub fn mono(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text).monospace().size(size).color(color)
}

/// Seven SMPTE bars in a square: the app icon.
pub fn app_icon(painter: &Painter, rect: Rect) {
    let w = rect.width() / theme::BARS.len() as f32;
    for (i, c) in theme::BARS.iter().enumerate() {
        let x = rect.left() + i as f32 * w;
        painter.rect_filled(Rect::from_min_max(pos2(x, rect.top()), pos2(x + w + 0.5, rect.bottom())), 0, *c);
    }
}

/// Colored rounded tile with the game's initials and an optional "running" dot.
pub fn game_tile(painter: &Painter, rect: Rect, game: Game, running: bool, dim: bool) {
    let (initials, rgb) = game.tile();
    let color = if dim { theme::LINE } else { theme::hex(rgb) };
    painter.rect_filled(rect, (rect.width() * 0.25) as u8, color);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials,
        FontId::proportional(rect.width() * 0.4),
        Color32::WHITE,
    );
    if running {
        painter.circle_filled(rect.right_bottom() - vec2(1.0, 1.0), 6.0, theme::SIDE);
        painter.circle_filled(rect.right_bottom() - vec2(1.0, 1.0), 4.0, theme::OK);
    }
}

/// Clickable full-width row with the design's hover and selected fills.
fn row_frame(ui: &mut egui::Ui, height: f32, selected: bool) -> (Rect, egui::Response) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let fill = match (selected, resp.hovered()) {
        (true, _) => theme::SELECTED,
        (false, true) => theme::HOVER,
        _ => Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, theme::RADIUS, fill);
    (rect, resp)
}

/// Game row in the sidebar: tile, title and a one-line subtitle. Returns true when clicked.
pub fn game_row(ui: &mut egui::Ui, game: Game, subtitle: &str, selected: bool, running: bool, off: bool) -> bool {
    let (rect, resp) = row_frame(ui, 46.0, selected);
    let p = ui.painter();
    let tile = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 14.0), Vec2::splat(28.0));
    game_tile(p, tile, game, running, off);
    let (x, y) = (tile.right() + 10.0, rect.center().y);
    let title = if selected { theme::TEXT } else { theme::FAINT };
    p.text(pos2(x, y - 8.0), Align2::LEFT_CENTER, game.title(), FontId::proportional(14.0), title);
    p.text(pos2(x, y + 9.0), Align2::LEFT_CENTER, subtitle, FontId::proportional(11.5), theme::MUTE);
    resp.clicked()
}

/// Navigation row with a leading glyph, e.g. "GPU scaling". Returns true when clicked.
pub fn nav_row(ui: &mut egui::Ui, glyph: &str, label: &str, selected: bool) -> bool {
    let (rect, resp) = row_frame(ui, 36.0, selected);
    let color = if selected { theme::TEXT } else { theme::FAINT };
    let (p, y) = (ui.painter(), rect.center().y);
    p.text(pos2(rect.left() + 20.0, y), Align2::CENTER_CENTER, glyph, FontId::proportional(15.0), color);
    p.text(pos2(rect.left() + 38.0, y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), color);
    resp.clicked()
}

/// Accent switch, 40 x 22 like the design. Returns true when toggled.
pub fn switch(ui: &mut egui::Ui, on: &mut bool, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    if resp.clicked() {
        *on = !*on;
    }
    let track = if *on { theme::ACCENT } else { theme::hex(0x4E5058) };
    ui.painter().rect_filled(rect, 11, if enabled { track } else { track.linear_multiply(0.55) });
    let x = if *on { rect.right() - 11.0 } else { rect.left() + 11.0 };
    ui.painter().circle_filled(pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp.clicked()
}

/// Settings row: title, subtitle and a trailing switch. Returns true when toggled.
pub fn setting_row(ui: &mut egui::Ui, title: &str, subtitle: &str, on: &mut bool, enabled: bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(48.0);
        ui.vertical(|ui| {
            ui.add_space(5.0);
            ui.label(RichText::new(title).color(theme::TEXT));
            ui.label(RichText::new(subtitle).size(12.0).color(theme::FAINT));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| changed = switch(ui, on, enabled));
    });
    changed
}

/// Status dot followed by a label, for the status bar.
pub fn status_dot(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
    ui.label(RichText::new(label).size(12.0).color(theme::FAINT));
}

/// Draws the demo game frame into `rect`. `squeeze` is how much the horizontal axis is
/// stretched: 1.0 for native 16:9, `native_aspect / target_aspect` for a stretched mode.
fn game_frame(p: &Painter, rect: Rect, squeeze: f32) {
    let c = rect.center();
    let (hw, hh) = (rect.width() / 2.0, rect.height() / 2.0);
    let at = |x: f32, y: f32| pos2(c.x + x * squeeze * hw, c.y + y * hh);
    let quad = |pts: [(f32, f32); 4], rgb: u32| {
        p.add(Shape::convex_polygon(pts.iter().map(|&(x, y)| at(x, y)).collect(), theme::hex(rgb), Stroke::NONE));
    };
    p.rect_filled(rect, 0, theme::hex(0x8FB0BE));
    quad([(-9.0, 0.08), (9.0, 0.08), (9.0, 1.0), (-9.0, 1.0)], 0x75634F);
    quad([(-1.4, -1.0), (-0.42, -0.42), (-0.42, 0.34), (-1.4, 1.0)], 0xA98764);
    quad([(1.4, -1.0), (0.42, -0.42), (0.42, 0.34), (1.4, 1.0)], 0x8C6F53);
    quad([(-0.42, -0.42), (0.42, -0.42), (0.42, 0.34), (-0.42, 0.34)], 0xCDBB9C);
    quad([(-0.12, -0.12), (0.12, -0.12), (0.12, 0.34), (-0.12, 0.34)], 0x2A2421);
    // Opponent in the doorway: body and head both widen when stretched.
    let body = Rect::from_center_size(at(0.0, 0.1), vec2(0.055 * squeeze * hw, 0.22 * hh));
    p.rect_filled(body, 4, theme::hex(0x2B3442));
    p.rect_stroke(body, 4, Stroke::new(1.5, theme::ERR), egui::StrokeKind::Outside);
    let head = at(0.0, -0.06);
    p.add(Shape::ellipse_filled(head, vec2(0.022 * squeeze * hw, 0.04 * hh), theme::hex(0xE6C3A2)));
    // The crosshair is rendered at game resolution, so its gap and arms stretch too.
    let (gap, arm, s) = (6.0 * squeeze, 9.0 * squeeze, Stroke::new(2.0, theme::hex(0x38FFD0)));
    for (a, b) in [(-gap - arm, -gap), (gap, gap + arm)] {
        p.line_segment([head + vec2(a, 0.0), head + vec2(b, 0.0)], s);
    }
}

/// The before/after widget: native on the left of the split, stretched `target` on the right.
/// Drag anywhere to move the split; `split` is a 0..1 fraction kept by the caller.
pub fn stretch_preview(ui: &mut egui::Ui, split: &mut f32, native: Resolution, target: Resolution) -> egui::Response {
    let width = ui.available_width().min(ui.available_height() * native.aspect());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, width / native.aspect()), Sense::click_and_drag());
    if let Some(pos) = resp.interact_pointer_pos() {
        *split = ((pos.x - rect.left()) / rect.width()).clamp(0.04, 0.96);
    }
    let squeeze = native.aspect() / target.aspect();
    let p = ui.painter_at(rect);
    let x = rect.left() + rect.width() * *split;
    game_frame(&p.with_clip_rect(Rect::from_min_max(rect.min, pos2(x, rect.bottom()))), rect, 1.0);
    game_frame(&p.with_clip_rect(Rect::from_min_max(pos2(x, rect.top()), rect.max)), rect, squeeze);
    p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0, Color32::WHITE));
    let knob = pos2(x, rect.top() + rect.height() * 0.82);
    p.circle_filled(knob, 14.0, Color32::WHITE);
    p.text(knob, Align2::CENTER_CENTER, "\u{2194}", FontId::proportional(15.0), theme::TITLE);
    let tag = |pos: Pos2, align: Align2, text: String, fill: Color32| {
        let galley = p.layout_no_wrap(text, display_font(11.0), Color32::BLACK);
        let r = align.anchor_size(pos, galley.size() + vec2(14.0, 10.0));
        p.rect_filled(r, 3, fill);
        p.galley(r.min + vec2(7.0, 5.0), galley, Color32::BLACK);
    };
    tag(rect.left_top() + vec2(14.0, 14.0), Align2::LEFT_TOP, format!("NATIVE {native}"), theme::TEXT);
    tag(rect.right_top() + vec2(-14.0, 14.0), Align2::RIGHT_TOP, format!("STRETCHED {target}"), theme::ACCENT);
    resp
}

#![windows_subsystem = "windows"]
//! TrueStretch: one-click true stretched resolution for VALORANT, Fortnite, CS2 and more.
//! Applies a per-game mode when the game starts and restores the desktop mode when it exits.

mod app;
mod display;
mod engine;
mod fonts;
mod games;
mod hotkey;
mod settings;
mod theme;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    // Crash watchdog: undo a stretched mode a previous run left behind.
    let recovered = engine::recover_after_crash();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = engine::recover_after_crash();
        default_hook(info);
    }));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TrueStretch")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([1024.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native("TrueStretch", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, recovered)))))
}

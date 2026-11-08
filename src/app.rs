//! Application state and layout: game list sidebar, per-game tabs, settings pages, status bar.

use crate::display::{self, Mode, Monitor};
use crate::engine::{Command, Engine, Event, Watch};
use crate::games::{Game, PatchReport};
use crate::hotkey::HotkeyListener;
use crate::settings::{PRESETS, Profile, Resolution, Settings};
use crate::theme;
use crate::widgets::{self, mono};
use eframe::egui::{self, Color32, RichText, Sense, vec2};
use std::time::{Duration, Instant};
use windows::Win32::System::SystemInformation::GetLocalTime;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Game(usize),
    Gpu,
    Safety,
}

const TABS: [&str; 4] = ["Profile", "Preview", "Config file", "Activity"];

pub struct App {
    settings: Settings,
    engine: Engine,
    hotkey: Option<HotkeyListener>,
    monitor: Option<Monitor>,
    device: String,
    modes: Vec<Mode>,
    page: Page,
    tab: usize,
    split: f32,
    running: Option<Game>,
    on_air: Option<Mode>,
    confirm_until: Option<Instant>,
    log: Vec<(String, Color32, String)>,
    patches: Vec<(Game, PatchReport)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, recovered: Option<String>) -> Self {
        theme::apply(&cc.egui_ctx);
        cc.egui_ctx.set_fonts(crate::fonts::definitions());
        let settings = Settings::load();
        let monitors = display::monitors();
        let monitor = monitors
            .iter()
            .find(|m| settings.monitor.as_deref() == Some(m.device.as_str()))
            .or_else(|| monitors.iter().find(|m| m.primary))
            .cloned();
        let device = monitor.as_ref().map_or_else(|| r"\\.\DISPLAY1".to_owned(), |m| m.device.clone());
        let mut app = Self {
            engine: Engine::spawn(cc.egui_ctx.clone(), Watch::new(&settings, device.clone())),
            hotkey: HotkeyListener::register(settings.hotkey, cc.egui_ctx.clone()),
            modes: display::modes(&device),
            settings,
            monitor,
            device,
            page: Page::Game(0),
            tab: 0,
            split: 0.5,
            running: None,
            on_air: None,
            confirm_until: None,
            log: Vec::new(),
            patches: Vec::new(),
        };
        if let Some(dev) = recovered {
            app.push(theme::ACCENT, format!("Restored the desktop resolution on {dev} after an unclean exit"));
        }
        if app.hotkey.is_none() {
            app.push(theme::ACCENT, format!("Hotkey {} is used by another program", app.settings.hotkey.label()));
        }
        app
    }

    fn native(&self) -> Resolution {
        self.monitor.as_ref().and_then(|m| m.native).map_or(Resolution::new(1920, 1080), Mode::resolution)
    }

    /// Adds an Activity entry stamped with the local time.
    fn push(&mut self, color: Color32, text: String) {
        // SAFETY: GetLocalTime has no preconditions.
        let t = unsafe { GetLocalTime() };
        self.log.insert(0, (format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond), color, text));
        self.log.truncate(200);
    }

    /// Saves settings and pushes the new profile set to the engine.
    fn commit(&mut self) {
        if let Err(e) = self.settings.save() {
            self.push(theme::ERR, format!("Could not save settings: {e}"));
        }
        self.engine.send(Command::Configure(Watch::new(&self.settings, self.device.clone())));
    }

    fn handle_events(&mut self) {
        for ev in self.engine.poll() {
            match ev {
                Event::GameStarted(g, pid) => {
                    self.running = Some(g);
                    self.push(theme::FAINT, format!("{} started ({} pid {pid})", g.title(), g.exe()));
                }
                Event::GameExited(g) => {
                    self.running = None;
                    self.push(theme::FAINT, format!("{} exited", g.title()));
                }
                Event::Applied(g, mode) => {
                    self.on_air = Some(mode);
                    // Manual tests must be confirmed; game launches use a saved profile.
                    let secs = Duration::from_secs(self.settings.confirm_seconds.into());
                    self.confirm_until = g.is_none().then(|| Instant::now() + secs);
                    self.push(theme::OK, format!("Applied {mode} (CDS_TEST, then CDS_FULLSCREEN)"));
                }
                Event::Reverted => {
                    (self.on_air, self.confirm_until) = (None, None);
                    self.push(theme::OK, "Restored desktop resolution from the registry".into());
                }
                Event::Patched(g, reports) => {
                    let n: usize = reports.iter().map(|r| r.changes.len()).sum();
                    self.push(theme::OK, format!("Patched {} config file: {n} keys", g.title()));
                    self.patches.retain(|(game, _)| *game != g);
                    self.patches.extend(reports.into_iter().map(|r| (g, r)));
                }
                Event::Error(e) => self.push(theme::ERR, e),
            }
        }
        if self.confirm_until.is_some_and(|t| Instant::now() >= t) {
            self.confirm_until = None;
            self.engine.send(Command::Revert);
        }
        if self.hotkey.as_ref().is_some_and(HotkeyListener::pressed) {
            let selected = match self.page {
                Page::Game(i) => self.settings.profiles.get(i).cloned(),
                _ => None,
            };
            match (self.on_air, selected) {
                (Some(_), _) => self.engine.send(Command::Revert),
                (None, Some(p)) => self.engine.send(Command::Apply(p)),
                _ => {}
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            widgets::app_icon(ui.painter(), icon);
            ui.label(RichText::new("TrueStretch").strong().color(theme::BODY));
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Games").size(11.5).strong().color(theme::FAINT));
        let mut select = None;
        for (i, p) in self.settings.profiles.iter().enumerate() {
            let sub = if p.enabled { format!("{} \u{00B7} auto", p.resolution) } else { "Profile off".into() };
            let running = self.running == Some(p.game);
            if widgets::game_row(ui, p.game, &sub, self.page == Page::Game(i), running, !p.enabled) {
                select = Some(Page::Game(i));
            }
        }
        ui.separator();
        if widgets::nav_row(ui, "\u{1F5A5}", "GPU scaling", self.page == Page::Gpu) {
            select = Some(Page::Gpu);
        }
        if widgets::nav_row(ui, "\u{1F512}", "Safety & hotkeys", self.page == Page::Safety) {
            select = Some(Page::Safety);
        }
        if let Some(page) = select {
            self.page = page;
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| self.monitor_panel(ui));
    }

    /// Monitor name, current mode and a Desktop / Stretched badge at the bottom of the sidebar.
    fn monitor_panel(&self, ui: &mut egui::Ui) {
        let Some(m) = &self.monitor else { return };
        let mode = self.on_air.or(m.current).map_or("unknown mode".into(), |c| c.to_string());
        let (badge, fill, ink) = match self.on_air {
            Some(_) => ("Stretched", theme::ACCENT, theme::TITLE),
            None => ("Desktop", theme::LINE, theme::BODY),
        };
        egui::Frame::new().fill(theme::hex(0x232428)).corner_radius(theme::RADIUS).inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&m.monitor_name).strong().color(theme::TEXT));
                    ui.label(RichText::new(mode).size(11.5).color(theme::FAINT));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::new()
                        .fill(fill)
                        .corner_radius(4)
                        .inner_margin(vec2(7.0, 3.0))
                        .show(ui, |ui| ui.label(RichText::new(badge).size(11.0).strong().color(ink)));
                });
            });
        });
    }

    fn game_page(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(p) = self.settings.profiles.get(i).cloned() else { return };
        egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (tile, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                widgets::game_tile(ui.painter(), tile, p.game, false, false);
                ui.vertical(|ui| {
                    ui.label(RichText::new(p.game.title()).size(17.0).strong().color(theme::TEXT));
                    let state = if self.running == Some(p.game) { "Running" } else { "Not running" };
                    ui.label(
                        RichText::new(format!("{} \u{00B7} {state}", p.game.exe())).size(12.5).color(theme::FAINT),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = RichText::new("Patch config now").strong().color(theme::TITLE);
                    if ui.add(egui::Button::new(label).fill(theme::ACCENT)).clicked() {
                        self.engine.send(Command::Patch(p.clone()));
                    }
                    if ui.button(format!("Test for {} s", self.settings.confirm_seconds)).clicked() {
                        self.engine.send(Command::Apply(p.clone()));
                    }
                    if self.on_air.is_some() && ui.button("Revert").clicked() {
                        self.engine.send(Command::Revert);
                    }
                });
            });
        });
        self.tabs(ui);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 12)).show(ui, |ui| match self.tab {
            0 => self.profile_tab(ui, i),
            1 => {
                ui.label(RichText::new("Drag the handle to compare").size(12.0).color(theme::FAINT));
                let native = self.native();
                widgets::stretch_preview(ui, &mut self.split, native, p.resolution);
            }
            2 => self.config_tab(ui, &p),
            _ => self.activity_tab(ui),
        });
    }

    /// Text tabs with an accent underline under the active one.
    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            for (t, name) in TABS.iter().enumerate() {
                let color = if self.tab == t { theme::TEXT } else { theme::FAINT };
                let r = ui.add(egui::Label::new(RichText::new(*name).size(14.0).color(color)).sense(Sense::click()));
                if self.tab == t {
                    let y = r.rect.bottom() + 9.0;
                    ui.painter().line_segment(
                        [egui::pos2(r.rect.left(), y), egui::pos2(r.rect.right(), y)],
                        (2.0, theme::ACCENT),
                    );
                }
                if r.clicked() {
                    self.tab = t;
                }
                ui.add_space(14.0);
            }
        });
        ui.add_space(8.0);
        ui.separator();
    }

    fn profile_tab(&mut self, ui: &mut egui::Ui, i: usize) {
        let modes = &self.modes;
        let Some(p) = self.settings.profiles.get_mut(i) else { return };
        let before = (p.resolution, p.enabled, p.patch_config, p.lock_config, p.gpu_stretch);
        ui.label(RichText::new("Stretched resolution").strong().color(theme::TEXT));
        ui.horizontal(|ui| {
            for res in PRESETS {
                ui.selectable_value(&mut p.resolution, res, mono(res.to_string(), 13.0, theme::BODY));
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Width").color(theme::FAINT));
            ui.add(egui::DragValue::new(&mut p.resolution.width).range(640..=7680));
            ui.label(RichText::new("Height").color(theme::FAINT));
            ui.add(egui::DragValue::new(&mut p.resolution.height).range(480..=4320));
            let (text, color) = display::pick(modes, p.resolution).map_or_else(
                || ("Not available: add it as a custom resolution".to_owned(), theme::ACCENT),
                |m| (format!("Driver mode {m}"), theme::OK),
            );
            ui.add_space(12.0);
            ui.label(RichText::new(text).size(12.5).color(color));
        });
        ui.add_space(14.0);
        ui.label(RichText::new(format!("When {} runs", p.game.title())).strong().color(theme::TEXT));
        let exe = p.game.exe();
        egui::Frame::new().fill(theme::SIDE).corner_radius(7).inner_margin(egui::Margin::symmetric(16, 6)).show(
            ui,
            |ui| {
                let auto = format!("Switches the display as soon as {exe} appears");
                widgets::setting_row(ui, "Apply automatically when the game starts", &auto, &mut p.enabled, true);
                let restore = "Always on. The desktop mode comes back from the registry";
                widgets::setting_row(ui, "Restore desktop resolution on exit", restore, &mut true, false);
                let edit = "Writes the resolution and letterbox keys to the game's settings file";
                widgets::setting_row(ui, "Edit game config to remove black bars", edit, &mut p.patch_config, true);
                let lock = "Stops the game from resetting the resolution after a patch";
                widgets::setting_row(ui, "Make config file read-only", lock, &mut p.lock_config, true);
                let gpu = "Requests full-panel scaling, falls back if the driver refuses";
                widgets::setting_row(ui, "Ask the GPU driver to stretch", gpu, &mut p.gpu_stretch, true);
            },
        );
        if before != (p.resolution, p.enabled, p.patch_config, p.lock_config, p.gpu_stretch) {
            self.commit();
        }
    }

    fn config_tab(&self, ui: &mut egui::Ui, p: &Profile) {
        if let Some(opts) = p.game.launch_options(p.resolution) {
            ui.label(mono(format!("Launch options: {opts}"), 12.5, theme::ACCENT));
        }
        let reports: Vec<&PatchReport> = self.patches.iter().filter(|(g, _)| *g == p.game).map(|(_, r)| r).collect();
        if reports.is_empty() {
            ui.label(
                RichText::new("Press \"Patch config now\" to see the changes made to the settings file.")
                    .color(theme::FAINT),
            );
        }
        for r in reports {
            let backup = r.backup.as_ref().map_or(String::new(), |b| format!("   backup {}", b.display()));
            ui.label(mono(format!("{}{backup}", r.file.display()), 12.0, theme::FAINT));
            egui::Frame::new().fill(theme::SIDE).corner_radius(6).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (k, old, new) in &r.changes {
                    if !old.is_empty() {
                        ui.label(mono(format!("- {k}={old}"), 12.5, theme::hex(0xF3A5A7)));
                    }
                    ui.label(mono(format!("+ {k}={new}"), 12.5, theme::hex(0x9FDCB7)));
                }
            });
        }
    }

    fn activity_tab(&self, ui: &mut egui::Ui) {
        if self.log.is_empty() {
            ui.label(RichText::new("Nothing yet. Launch a game with a profile to see it here.").color(theme::FAINT));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (time, color, text) in &self.log {
                ui.horizontal(|ui| {
                    ui.label(mono(time, 12.0, theme::FAINT));
                    widgets::status_dot(ui, *color, "");
                    ui.label(text);
                });
            }
        });
    }

    fn gpu_page(&self, ui: &mut egui::Ui) {
        let Some(m) = &self.monitor else {
            ui.label("No monitor attached to the desktop was found.");
            return;
        };
        egui::Frame::new().fill(theme::SIDE).corner_radius(7).inner_margin(14).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&m.adapter).strong().color(theme::TEXT));
            ui.label(RichText::new(format!("{:?} graphics", m.vendor)).size(12.0).color(theme::FAINT));
            ui.separator();
            ui.label(RichText::new(&m.monitor_name).strong().color(theme::TEXT));
            let mode = m.native.map_or(String::new(), |n| n.to_string());
            ui.label(RichText::new(format!("{} \u{00B7} {mode}", m.device)).size(12.0).color(theme::FAINT));
        });
        ui.add_space(12.0);
        ui.label(RichText::new("Set scaling to full-screen").strong().color(theme::TEXT));
        for step in m.vendor.scaling_steps() {
            ui.label(format!("\u{2022}  {step}"));
        }
        ui.add_space(12.0);
        ui.label(RichText::new(format!("Modes on {}", m.device)).strong().color(theme::TEXT));
        for res in PRESETS {
            let found = display::pick(&self.modes, res);
            let (txt, col) =
                found.map_or(("Not available".into(), theme::ACCENT), |m| (format!("{} Hz", m.hz), theme::OK));
            ui.horizontal(|ui| {
                ui.add_sized([110.0, 20.0], egui::Label::new(mono(res.to_string(), 13.0, theme::BODY)));
                ui.label(RichText::new(txt).color(col));
            });
        }
    }

    fn safety_page(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.settings;
        let mut changed = false;
        egui::Frame::new().fill(theme::SIDE).corner_radius(7).inner_margin(egui::Margin::symmetric(16, 10)).show(
            ui,
            |ui| {
                ui.set_width(ui.available_width().min(760.0));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Confirm manual changes within").color(theme::TEXT));
                    changed |= ui.add(egui::Slider::new(&mut s.confirm_seconds, 5..=60).suffix(" s")).changed();
                });
                let auto = "Checks running processes for the games in your list";
                changed |= widgets::setting_row(ui, "Apply profiles automatically", auto, &mut s.auto_apply, true);
                let crash = "A stretched mode left behind is reverted on the next start";
                widgets::setting_row(ui, "Crash recovery", crash, &mut true, false);
                ui.label(RichText::new(format!("Toggle stretch: {}", s.hotkey.label())).color(theme::TEXT));
            },
        );
        ui.add_space(12.0);
        if self.on_air.is_some() && ui.button("Revert now").clicked() {
            self.engine.send(Command::Revert);
        }
        if changed {
            self.commit();
        }
    }

    /// "Keep these display settings?" dialog shown after a manual test.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(until) = self.confirm_until else { return };
        let left = until.saturating_duration_since(Instant::now());
        let total = self.settings.confirm_seconds.max(1) as f32;
        let mode = self.on_air.map_or(String::new(), |m| m.to_string());
        egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new("Keep these display settings?").size(17.0).strong().color(theme::TEXT));
            ui.label(format!("{mode}. Reverting to previous display settings in {} seconds.", left.as_secs() + 1));
            ui.add(egui::ProgressBar::new(left.as_secs_f32() / total).desired_height(4.0).fill(theme::ACCENT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let keep = RichText::new("Keep changes").strong().color(theme::TITLE);
                if ui.add(egui::Button::new(keep).fill(theme::ACCENT)).clicked() {
                    self.confirm_until = None;
                }
                if ui.button("Revert").clicked() {
                    self.engine.send(Command::Revert);
                }
            });
        });
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let armed = self.settings.profiles.iter().filter(|p| p.enabled).count();
            let watching = if self.settings.auto_apply { theme::OK } else { theme::MUTE };
            widgets::status_dot(ui, watching, &format!("Watching {armed} games"));
            ui.add_space(12.0);
            let mode = self.on_air.map_or_else(|| format!("{} desktop", self.native()), |m| format!("{m} stretched"));
            ui.label(RichText::new(format!("{} \u{00B7} {mode}", self.device)).size(12.0).color(theme::FAINT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(concat!("v", env!("CARGO_PKG_VERSION"))).size(12.0).color(theme::FAINT));
                let hotkey = format!("{} toggles stretch", self.settings.hotkey.label());
                ui.label(RichText::new(hotkey).size(12.0).color(theme::FAINT));
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_events();
        let ctx = ui.ctx().clone();
        let bar = egui::Frame::new().fill(theme::TITLE).inner_margin(egui::Margin::symmetric(12, 0));
        egui::Panel::bottom("status").exact_size(26.0).frame(bar).show(ui, |ui| self.status_bar(ui));
        let side = egui::Frame::new().fill(theme::SIDE).inner_margin(8);
        egui::Panel::left("games").exact_size(248.0).frame(side).show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::MAIN)).show(ui, |ui| match self.page {
            Page::Game(i) => self.game_page(ui, i),
            page => {
                let title = if page == Page::Gpu { "GPU scaling" } else { "Safety & hotkeys" };
                egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 18)).show(ui, |ui| {
                    ui.label(RichText::new(title).size(17.0).strong().color(theme::TEXT));
                    ui.add_space(12.0);
                    if page == Page::Gpu { self.gpu_page(ui) } else { self.safety_page(ui) }
                });
            }
        });
        self.confirm_dialog(&ctx);
    }
}

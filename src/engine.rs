//! The stretch engine: one background thread that polls running processes with a ToolHelp
//! snapshot, applies a game's profile when it starts and restores the desktop mode when it exits.
//! The UI talks to it only through channels, so the UI thread never blocks on GDI or disk.

use crate::display::{self, Mode};
use crate::games::{Game, PatchReport};
use crate::settings::{Profile, Settings};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::*;

/// What the watcher looks for, and where it applies modes.
#[derive(Clone)]
pub struct Watch {
    pub profiles: Vec<Profile>,
    /// GDI device name of the target monitor, e.g. `\\.\DISPLAY1`.
    pub device: String,
    pub poll: Duration,
    /// Apply profiles automatically when their game starts.
    pub auto: bool,
}

impl Watch {
    pub fn new(s: &Settings, device: String) -> Self {
        let poll = Duration::from_millis(s.poll_ms.max(200));
        Self { profiles: s.profiles.clone(), device, poll, auto: s.auto_apply }
    }
}

/// Requests from the UI.
pub enum Command {
    Configure(Watch),
    /// Apply a profile right now (manual test or hotkey); the UI then asks for confirmation.
    Apply(Profile),
    /// Patch the game's config files without touching the display.
    Patch(Profile),
    /// Restore the desktop mode.
    Revert,
}

/// Notifications for the UI.
pub enum Event {
    GameStarted(Game, u32),
    GameExited(Game),
    /// A stretched mode is on air; `None` means a manual test rather than a game launch.
    Applied(Option<Game>, Mode),
    Reverted,
    Patched(Game, Vec<PatchReport>),
    Error(String),
}

/// Handle to the engine thread. Dropping it reverts any active mode and joins the thread.
pub struct Engine {
    tx: Option<Sender<Command>>,
    events: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn spawn(ctx: egui::Context, watch: Watch) -> Self {
        let (tx, rx) = mpsc::channel();
        let (out, events) = mpsc::channel();
        let mut worker = Worker { watch, active: None, stretched: false, out, ctx };
        let handle = std::thread::Builder::new().name("stretch-engine".into()).spawn(move || worker.run(rx)).ok();
        Self { tx: Some(tx), events, handle }
    }

    pub fn send(&self, cmd: Command) {
        // A closed channel only happens during shutdown; nothing is left to do then.
        let _ = self.tx.as_ref().map(|tx| tx.send(cmd));
    }

    /// Drains every pending event without blocking.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Worker {
    watch: Watch,
    /// The game currently driving a stretched mode, with its process id.
    active: Option<(Game, u32)>,
    stretched: bool,
    out: Sender<Event>,
    ctx: egui::Context,
}

impl Worker {
    fn emit(&self, e: Event) {
        let _ = self.out.send(e);
        self.ctx.request_repaint();
    }

    fn run(&mut self, rx: Receiver<Command>) {
        loop {
            match rx.recv_timeout(self.watch.poll) {
                Ok(Command::Configure(w)) => self.watch = w,
                Ok(Command::Apply(p)) => self.apply(&p, None),
                Ok(Command::Patch(p)) => self.patch(&p),
                Ok(Command::Revert) => self.revert(),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if self.watch.auto {
                self.tick();
            }
        }
        if self.stretched {
            self.revert();
        }
    }

    /// One watcher tick: detect the start or exit of the first enabled game.
    fn tick(&mut self) {
        let procs = match running_processes() {
            Ok(p) => p,
            Err(e) => return self.emit(Event::Error(format!("process snapshot failed: {e}"))),
        };
        if let Some((game, pid)) = self.active {
            if !procs.iter().any(|(p, _)| *p == pid) {
                self.active = None;
                self.emit(Event::GameExited(game));
                self.revert();
                // VALORANT and Fortnite rewrite their ini on exit; patch again for the next launch.
                if let Some(p) = self.watch.profiles.iter().find(|p| p.game == game && p.patch_config).cloned() {
                    self.patch(&p);
                }
            }
            return;
        }
        let hit = self.watch.profiles.iter().filter(|p| p.enabled).find_map(|p| {
            procs.iter().find(|(_, exe)| exe.eq_ignore_ascii_case(p.game.exe())).map(|(pid, _)| (p.clone(), *pid))
        });
        if let Some((profile, pid)) = hit {
            self.active = Some((profile.game, pid));
            self.emit(Event::GameStarted(profile.game, pid));
            self.apply(&profile, Some(profile.game));
        }
    }

    fn apply(&mut self, p: &Profile, game: Option<Game>) {
        let device = self.watch.device.clone();
        let Some(mode) = display::pick(&display::modes(&device), p.resolution) else {
            let msg =
                format!("{} is not exposed by the driver on {device}; add it as a custom resolution", p.resolution);
            return self.emit(Event::Error(msg));
        };
        if let Err(e) = display::apply(&device, mode, p.gpu_stretch) {
            return self.emit(Event::Error(e.to_string()));
        }
        self.stretched = true;
        if let Err(e) = std::fs::create_dir_all(Settings::dir()).and_then(|_| std::fs::write(marker_path(), &device)) {
            self.emit(Event::Error(format!("could not write the crash-watchdog marker: {e}")));
        }
        self.emit(Event::Applied(game, mode));
    }

    fn revert(&mut self) {
        if let Err(e) = display::revert(&self.watch.device) {
            return self.emit(Event::Error(e.to_string()));
        }
        self.stretched = false;
        let _ = std::fs::remove_file(marker_path());
        self.emit(Event::Reverted);
    }

    fn patch(&self, p: &Profile) {
        let patcher = p.game.patcher();
        let files = patcher.locate();
        if files.is_empty() {
            return self.emit(Event::Error(format!("no {} settings file found; launch the game once", p.game.title())));
        }
        let mut reports = Vec::new();
        for f in files {
            match patcher.patch(&f, p.resolution, p.lock_config) {
                Ok(r) => reports.push(r),
                Err(e) => self.emit(Event::Error(format!("{}: {e}", f.display()))),
            }
        }
        self.emit(Event::Patched(p.game, reports));
    }
}

/// `(pid, exe name)` for every running process.
pub fn running_processes() -> windows::core::Result<Vec<(u32, String)>> {
    // SAFETY: the snapshot handle is closed below; the entry has `dwSize` set as required.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut out = Vec::with_capacity(256);
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            out.push((entry.th32ProcessID, String::from_utf16_lossy(&entry.szExeFile[..len])));
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        CloseHandle(snap)?;
        Ok(out)
    }
}

/// Records which monitor is stretched, so a crash or forced kill can be repaired on next start.
fn marker_path() -> PathBuf {
    Settings::dir().join("applied.marker")
}

/// Crash watchdog, run at startup and from the panic hook: if a stretched mode was left behind,
/// restore the desktop mode of that monitor. Returns the device that was repaired.
pub fn recover_after_crash() -> Option<String> {
    let device = std::fs::read_to_string(marker_path()).ok()?.trim().to_owned();
    display::revert(&device).ok()?;
    let _ = std::fs::remove_file(marker_path());
    Some(device)
}

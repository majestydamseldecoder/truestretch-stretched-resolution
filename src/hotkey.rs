//! Global hotkey (default Ctrl + Alt + F9) on its own message-loop thread.

use crate::settings::Hotkey;
use eframe::egui;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

const MOD_NOREPEAT: u32 = 0x4000;
const HOTKEY_ID: i32 = 0x5453; // "TS"

// RegisterHotKey lives in Win32_UI_Input_KeyboardAndMouse; it is declared here directly so the
// shared `windows` feature list stays untouched.
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterHotKey(hwnd: *mut c_void, id: i32, modifiers: u32, vk: u32) -> i32;
}

/// A global hotkey registered for the life of the process; poll [`HotkeyListener::pressed`]
/// once per frame. Windows releases the registration when the process exits.
pub struct HotkeyListener {
    presses: Receiver<()>,
}

impl HotkeyListener {
    /// Registers `key`; returns `None` if another program already owns the combination.
    pub fn register(key: Hotkey, ctx: egui::Context) -> Option<Self> {
        let (tx, presses) = mpsc::channel();
        let (ok_tx, ok_rx) = mpsc::channel();
        let thread = std::thread::Builder::new().name("hotkey".into()).spawn(move || {
            // SAFETY: plain Win32 calls on this thread's own message queue (hotkeys registered
            // with a null window are posted to the registering thread).
            unsafe {
                let ok = RegisterHotKey(std::ptr::null_mut(), HOTKEY_ID, key.modifiers | MOD_NOREPEAT, key.vk) != 0;
                let _ = ok_tx.send(ok);
                let mut msg = MSG::default();
                while ok && GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_HOTKEY && tx.send(()).is_ok() {
                        ctx.request_repaint();
                    }
                }
            }
        });
        (thread.is_ok() && ok_rx.recv() == Ok(true)).then_some(Self { presses })
    }

    /// True if the hotkey was pressed since the last call.
    pub fn pressed(&self) -> bool {
        self.presses.try_iter().count() > 0
    }
}

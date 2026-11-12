//! Window operations a client-side-decorated (transparent titlebar) window
//! needs, per platform.
//!
//! * **Windows** — gpui 0.2.2 can't drag or restore a frameless window from the
//!   client area, so we go through Win32: `WM_NCLBUTTONDOWN`/`HTCAPTION` starts
//!   the native caption drag, and `WM_SYSCOMMAND` + `SC_MAXIMIZE`/`SC_RESTORE`
//!   is the exact path the taskbar button and Win+Up use — which is what makes
//!   a frameless maximize render correctly (titlebar stays visible, no overlap
//!   of the taskbar). We `PostMessageW` (queued) rather than `SendMessageW`, so
//!   it runs in the normal message loop like an OS-initiated maximize.
//! * **macOS / Linux** — gpui's own methods. On macOS the transparent native
//!   titlebar strip is already draggable, so `start_window_move` is a harmless
//!   no-op there; on Wayland/X11 it asks the compositor to take over the move.

use gpui::Window;

#[cfg(windows)]
mod imp {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{
        PostMessageW, ReleaseCapture, SendMessageW, HTCAPTION, SC_MAXIMIZE, SC_RESTORE,
        WM_NCLBUTTONDOWN, WM_SYSCOMMAND,
    };

    fn hwnd(window: &Window) -> Option<HWND> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
            _ => None,
        }
    }

    pub fn start_drag(window: &mut Window) {
        if let Some(h) = hwnd(window) {
            unsafe {
                // gpui holds the mouse capture; the caption drag needs it back.
                ReleaseCapture();
                SendMessageW(h, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
            }
        }
    }

    pub fn maximize(window: &mut Window) {
        if let Some(h) = hwnd(window) {
            unsafe {
                // System commands are ignored while a capture is held.
                ReleaseCapture();
                PostMessageW(h, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0);
            }
        }
    }

    pub fn restore(window: &mut Window) {
        if let Some(h) = hwnd(window) {
            unsafe {
                ReleaseCapture();
                PostMessageW(h, WM_SYSCOMMAND, SC_RESTORE as usize, 0);
            }
        }
    }
}

#[cfg(windows)]
pub fn start_drag(window: &mut Window) {
    imp::start_drag(window);
}

#[cfg(windows)]
pub fn maximize(window: &mut Window) {
    imp::maximize(window);
}

#[cfg(windows)]
pub fn restore(window: &mut Window) {
    imp::restore(window);
}

#[cfg(not(windows))]
pub fn start_drag(window: &mut Window) {
    window.start_window_move();
}

#[cfg(not(windows))]
pub fn maximize(window: &mut Window) {
    window.zoom_window();
}

#[cfg(not(windows))]
pub fn restore(window: &mut Window) {
    window.zoom_window();
}

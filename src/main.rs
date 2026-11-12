//! RustedPdf — a cross-platform PDF reader and highlighter built with GPUI.
//!
//! * [`ui::titlebar`] — the reusable custom titlebar (nav menu + window buttons
//!   on one strip); nothing in it is PDF-specific.
//! * [`pdf`] — PDFium binding, page rasterising, text selection and marks.
//! * [`app`] — workspace state and commands. [`view`] — how it's drawn.
//! * [`state`] — settings, recent files and the crash journal.

// GUI app: don't spawn a console window on Windows.
#![windows_subsystem = "windows"]

mod app;
mod integration;
mod menu;
mod pdf;
mod state;
mod tab;
mod theme;
mod ui;
mod view;

use gpui::{
    prelude::*, px, size, App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions,
};

use app::{
    CloseTab, CopySelection, Dismiss, ExportPages, GoToPage, NextTab, OpenFile, OpenSettings,
    PrevTab, SaveAll, SaveFile, SaveFileAs, SelectAll, ToggleTheme, ToolErase, ToolHighlight,
    ToolSelect, ToolUnderline, UndoMark, Workspace, ZoomIn, ZoomOut, ZoomReset,
};
use ui::titlebar::{window_options_titlebar, TitleBarMetrics};

/// Ctrl on Windows/Linux, Cmd on macOS.
#[cfg(target_os = "macos")]
const MOD: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "ctrl";

/// Shortcuts only fire in the "Workspace" key context, which the root view
/// swaps for "Modal" while a dialog is up — that's what stops them reaching
/// the document behind a modal. Escape is bound in both.
fn key_bindings() -> Vec<KeyBinding> {
    let workspace = Some("Workspace");

    vec![
        KeyBinding::new(&format!("{MOD}-o"), OpenFile, workspace),
        KeyBinding::new(&format!("{MOD}-s"), SaveFile, workspace),
        KeyBinding::new(&format!("{MOD}-shift-s"), SaveFileAs, workspace),
        KeyBinding::new(&format!("{MOD}-alt-s"), SaveAll, workspace),
        KeyBinding::new(&format!("{MOD}-w"), CloseTab, workspace),
        KeyBinding::new("ctrl-tab", NextTab, workspace),
        KeyBinding::new("ctrl-shift-tab", PrevTab, workspace),
        KeyBinding::new(&format!("{MOD}-="), ZoomIn, workspace),
        KeyBinding::new(&format!("{MOD}-+"), ZoomIn, workspace),
        KeyBinding::new(&format!("{MOD}--"), ZoomOut, workspace),
        KeyBinding::new(&format!("{MOD}-0"), ZoomReset, workspace),
        KeyBinding::new(&format!("{MOD}-1"), ToolSelect, workspace),
        KeyBinding::new(&format!("{MOD}-2"), ToolHighlight, workspace),
        KeyBinding::new(&format!("{MOD}-3"), ToolUnderline, workspace),
        KeyBinding::new(&format!("{MOD}-4"), ToolErase, workspace),
        KeyBinding::new(&format!("{MOD}-z"), UndoMark, workspace),
        KeyBinding::new(&format!("{MOD}-c"), CopySelection, workspace),
        KeyBinding::new(&format!("{MOD}-a"), SelectAll, workspace),
        KeyBinding::new(&format!("{MOD}-e"), ExportPages, workspace),
        KeyBinding::new(&format!("{MOD}-g"), GoToPage, workspace),
        KeyBinding::new(&format!("{MOD}-,"), OpenSettings, workspace),
        KeyBinding::new(&format!("{MOD}-shift-t"), ToggleTheme, workspace),
        KeyBinding::new("escape", Dismiss, workspace),
        KeyBinding::new("escape", Dismiss, Some("Modal")),
    ]
}

/// Switches that do one job and exit, before any window exists. Returns the
/// process exit code when one was handled.
///
/// `--unregister-shell` is what the uninstaller calls. The "Open with" entries
/// the installer writes are its own and go when it does, but the ones
/// Settings ▸ File manager writes are per-user, under `HKEY_CURRENT_USER`,
/// where a per-machine uninstall can't see them. Left behind they name an
/// executable that no longer exists — a dead RustedPdf sitting in Explorer's
/// menu for PDFs. Asking the app to undo its own registration is the only
/// thing that knows where to look.
fn handle_switch() -> Option<i32> {
    let switch = std::env::args().nth(1)?;

    let result = match switch.as_str() {
        "--register-shell" => integration::register(),
        "--unregister-shell" => integration::unregister(),
        _ => return None,
    };

    Some(i32::from(result.is_err()))
}

fn main() {
    if let Some(code) = handle_switch() {
        std::process::exit(code);
    }

    Application::new().run(|cx: &mut App| {
        cx.bind_keys(key_bindings());

        // Files passed on the command line, e.g. Windows "Open with…".
        let initial: Vec<_> = std::env::args()
            .skip(1)
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_file())
            .collect();

        let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);

        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // Transparent titlebar = client-side decorations: the app
                    // draws the whole strip itself (see `ui::titlebar`).
                    titlebar: Some(window_options_titlebar(
                        "RustedPdf",
                        TitleBarMetrics::default().height,
                    )),
                    // Below this the nav would collide with the window buttons.
                    window_min_size: Some(size(px(640.0), px(420.0))),
                    ..Default::default()
                },
                |_, cx| cx.new(Workspace::new),
            )
            .expect("failed to open window");

        window
            .update(cx, |workspace, window, cx| {
                let handle = workspace.focus_handle.clone();
                window.focus(&handle);

                for path in initial {
                    workspace.open_path(&path, window, cx);
                }

                // Repaint when the OS light/dark appearance changes.
                window
                    .observe_window_appearance(|window, _| window.refresh())
                    .detach();
            })
            .ok();

        cx.activate(true);
    });
}

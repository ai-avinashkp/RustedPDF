//! RustedPdf's menu tree.
//!
//! Data only — see [`crate::ui::titlebar::menu`] for the model and
//! [`crate::app::Workspace::on_menu_command`] for what the ids do.

use crate::ui::titlebar::{cmd, Item, Menu, SEP};

// --- File ----------------------------------------------------------------

const FILE: &[Item] = &[
    cmd("file.open", "Open PDF…", Some("Ctrl+O")),
    cmd("file.export", "Export Pages…", Some("Ctrl+E")),
    SEP,
    cmd("file.save", "Save", Some("Ctrl+S")),
    cmd("file.save_as", "Save a Copy…", Some("Ctrl+Shift+S")),
    cmd("file.save_all", "Save All", Some("Ctrl+Alt+S")),
    SEP,
    cmd("file.close_tab", "Close Tab", Some("Ctrl+W")),
    cmd("file.exit", "Exit", Some("Alt+F4")),
];

// --- Edit ----------------------------------------------------------------

const EDIT: &[Item] = &[
    cmd("edit.copy", "Copy Selected Text", Some("Ctrl+C")),
    cmd("edit.select_all", "Select All", Some("Ctrl+A")),
    SEP,
    cmd("edit.undo_annotation", "Undo Last Mark", Some("Ctrl+Z")),
    SEP,
    cmd("edit.clear_page", "Clear Marks on Page", None),
    cmd("edit.clear_document", "Clear Marks in Document", None),
    SEP,
    cmd("edit.settings", "Settings…", Some("Ctrl+,")),
];

// --- View ----------------------------------------------------------------

const VIEW: &[Item] = &[
    cmd("view.go_to_page", "Go to Page…", Some("Ctrl+G")),
    cmd("view.first_page", "First Page", None),
    cmd("view.last_page", "Last Page", None),
    SEP,
    cmd("view.zoom_in", "Zoom In", Some("Ctrl++")),
    cmd("view.zoom_out", "Zoom Out", Some("Ctrl+-")),
    cmd("view.zoom_reset", "Actual Size (100%)", Some("Ctrl+0")),
    SEP,
    cmd("view.zoom.50", "50%", None),
    cmd("view.zoom.75", "75%", None),
    cmd("view.zoom.100", "100%", None),
    cmd("view.zoom.150", "150%", None),
    cmd("view.zoom.200", "200%", None),
    SEP,
    cmd("view.theme", "Toggle Dark Mode", Some("Ctrl+Shift+T")),
];

// --- Tools ---------------------------------------------------------------

const TOOLS: &[Item] = &[
    cmd("tools.select", "Select / Pan", Some("Ctrl+1")),
    cmd("tools.highlight", "Highlighter", Some("Ctrl+2")),
    cmd("tools.underline", "Underline", Some("Ctrl+3")),
    cmd("tools.erase", "Eraser", Some("Ctrl+4")),
    SEP,
    cmd("tools.color", "Mark Colour…", None),
];

// --- Help ----------------------------------------------------------------

const HELP: &[Item] = &[
    cmd("help.shortcuts", "Keyboard Shortcuts", None),
    cmd("help.about", "About RustedPdf", None),
];

/// The menu bar, left to right.
pub const MENUS: &[Menu] = &[
    Menu {
        label: "File",
        items: FILE,
    },
    Menu {
        label: "Edit",
        items: EDIT,
    },
    Menu {
        label: "View",
        items: VIEW,
    },
    Menu {
        label: "Tools",
        items: TOOLS,
    },
    Menu {
        label: "Help",
        items: HELP,
    },
];

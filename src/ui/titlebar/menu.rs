//! The menu tree, as data.
//!
//! A host app declares its menus as `&'static [Menu]` using the `cmd` / `sub` /
//! `SEP` helpers; [`super::TitleBar`] renders any depth of nesting, so adding a
//! submenu never touches UI code.
//!
//! ```ignore
//! const ROTATE: &[Item] = &[
//!     cmd("view.rotate.cw", "Clockwise", Some("Ctrl+]")),
//! ];
//! const VIEW: &[Item] = &[
//!     cmd("view.zoom_in", "Zoom In", Some("Ctrl++")),
//!     SEP,
//!     sub("Rotate", ROTATE),
//! ];
//! ```

#[derive(Debug, Clone, Copy)]
pub struct Menu {
    /// Top-level label shown on the bar.
    pub label: &'static str,
    pub items: &'static [Item],
}

#[derive(Debug, Clone, Copy)]
pub enum Item {
    /// A leaf command. `id` is what the host dispatches on.
    Command {
        id: &'static str,
        label: &'static str,
        shortcut: Option<&'static str>,
        enabled: bool,
    },
    /// A command that reflects on/off state, drawn with a check mark.
    Toggle {
        id: &'static str,
        label: &'static str,
        shortcut: Option<&'static str>,
        checked: bool,
        enabled: bool,
    },
    Separator,
    Submenu {
        label: &'static str,
        items: &'static [Item],
    },
}

/// A clickable command.
pub const fn cmd(id: &'static str, label: &'static str, shortcut: Option<&'static str>) -> Item {
    Item::Command {
        id,
        label,
        shortcut,
        enabled: true,
    }
}

/// A command that is visible but not invokable — greyed out.
pub const fn disabled(
    id: &'static str,
    label: &'static str,
    shortcut: Option<&'static str>,
) -> Item {
    Item::Command {
        id,
        label,
        shortcut,
        enabled: false,
    }
}

/// A checkable command. Static menus can't know runtime state, so hosts that
/// need a live check mark build their `Menu` list dynamically.
pub const fn toggle(
    id: &'static str,
    label: &'static str,
    shortcut: Option<&'static str>,
    checked: bool,
) -> Item {
    Item::Toggle {
        id,
        label,
        shortcut,
        checked,
        enabled: true,
    }
}

pub const fn sub(label: &'static str, items: &'static [Item]) -> Item {
    Item::Submenu { label, items }
}

pub const SEP: Item = Item::Separator;

/// Tracks which dropdown — and which chain of submenus inside it — is open.
#[derive(Debug, Default)]
pub struct MenuState {
    /// Index into the host's menu list of the open dropdown, if any.
    pub(super) open: Option<usize>,
    /// One item index per open nesting level, outermost first.
    pub(super) path: Vec<usize>,
}

impl MenuState {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Closes everything. Returns whether anything actually changed, so callers
    /// can skip a repaint.
    pub fn close(&mut self) -> bool {
        let was_open = self.open.is_some();
        self.open = None;
        self.path.clear();
        was_open
    }

    pub(super) fn toggle(&mut self, index: usize) {
        if self.open == Some(index) {
            self.close();
        } else {
            self.open = Some(index);
            self.path.clear();
        }
    }

    pub(super) fn switch_to(&mut self, index: usize) {
        self.open = Some(index);
        self.path.clear();
    }

    /// Opens submenu `index` at nesting `depth`, closing anything deeper.
    pub(super) fn open_submenu(&mut self, depth: usize, index: usize) {
        self.path.truncate(depth);
        self.path.push(index);
    }

    pub(super) fn close_submenus_below(&mut self, depth: usize) {
        self.path.truncate(depth);
    }
}

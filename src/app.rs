//! The workspace: open tabs, the active tool, modals, and everything the menu
//! and keyboard dispatch into.
//!
//! Rendering lives in [`crate::view`]; this module is state and behaviour.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui::{
    actions, px, ClipboardItem, Context, FocusHandle, Focusable, Pixels, Point, SharedString,
    Window,
};

use crate::integration;
use crate::pdf::export::{self, ExportFormat};
use crate::pdf::{Document, MarkKind};
use crate::state::{Journal, Recents, Settings};
use crate::tab::{autoscroll_velocity, Tab, Tool};
use crate::theme::{Palette, ThemeMode};
use crate::ui::titlebar::{TitleBarHost, TitleBarState};

actions!(
    rusted_pdf,
    [
        OpenFile,
        SaveFile,
        SaveFileAs,
        SaveAll,
        CloseTab,
        NextTab,
        PrevTab,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ToolSelect,
        ToolHighlight,
        ToolUnderline,
        ToolErase,
        UndoMark,
        CopySelection,
        SelectAll,
        ExportPages,
        GoToPage,
        ToggleTheme,
        OpenSettings,
        Dismiss,
    ]
);

/// UI font, per platform.
#[cfg(target_os = "windows")]
pub const UI_FONT: &str = "Segoe UI";
#[cfg(target_os = "macos")]
pub const UI_FONT: &str = "Helvetica Neue";
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
pub const UI_FONT: &str = "sans-serif";

/// Paint layers. The menu sits at [`crate::ui::titlebar::MENU_LAYER`] (2); a
/// modal has to cover it, and its backdrop has to cover everything else.
pub const BACKDROP_LAYER: usize = 1;
pub const MODAL_LAYER: usize = 9;

/// Which files a close request is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseScope {
    /// The whole window: everything unsaved is on the table.
    Window,
    /// One tab.
    Tab(usize),
}

/// A selection drag pulling the viewport after it.
///
/// The pointer position is kept because auto-scroll has to carry on while the
/// mouse is perfectly still — held just past the bottom edge is the normal way
/// to do this — and there are no move events to read it from. `stepped` is
/// what makes the speed a rate rather than a per-frame nudge.
#[derive(Debug, Clone, Copy)]
pub struct DragScroll {
    pub pointer: Point<Pixels>,
    pub stepped: Instant,
}

/// The right-click menu over the document.
#[derive(Debug, Clone, Copy)]
pub struct ContextMenu {
    /// Where it was opened, in window coordinates — the menu's top-left.
    pub position: Point<Pixels>,
    /// The page and point that was right-clicked, if it was over a page, so
    /// "Remove Mark Here" knows what "here" is.
    pub target: Option<(usize, (f32, f32))>,
}

/// An eraser being dragged across the page.
#[derive(Debug, Clone, Copy)]
pub struct EraseDrag {
    /// Where the pointer was at the last move. Moves arrive tens of pixels
    /// apart when the pointer is quick, so the eraser sweeps the whole
    /// segment between two of them rather than only the points it is told
    /// about — otherwise a fast stroke skips straight over a thin underline.
    pub last: Point<Pixels>,
    /// Marks removed so far in this stroke.
    pub removed: usize,
}

/// How long after the last pinch or Ctrl+wheel step the zoom counts as
/// settled, and the pages are rendered sharp at the new scale.
pub const PINCH_SETTLE: std::time::Duration = std::time::Duration::from_millis(180);

/// A row in the save-before-closing dialog.
#[derive(Debug, Clone)]
pub struct CloseTarget {
    pub tab: usize,
    pub name: String,
    pub marks: usize,
    /// Ticked rows are what "Save Selected" writes.
    pub selected: bool,
}

pub enum Modal {
    Settings,
    About,
    Shortcuts,
    /// Errors and notices — anything that just needs an OK.
    Message {
        title: SharedString,
        body: SharedString,
    },
    ConfirmClose {
        scope: CloseScope,
        targets: Vec<CloseTarget>,
    },
    /// Pick pages and a format, then write them out.
    Export {
        /// One flag per page of the active document.
        pages: Vec<bool>,
        format: ExportFormat,
    },
}

impl Modal {
    pub fn title(&self) -> SharedString {
        match self {
            Modal::Settings => "Settings".into(),
            Modal::About => "About RustedPdf".into(),
            Modal::Shortcuts => "Keyboard Shortcuts".into(),
            Modal::Message { title, .. } => title.clone(),
            Modal::ConfirmClose { .. } => "Unsaved marks".into(),
            Modal::Export { .. } => "Export pages".into(),
        }
    }
}

pub struct Workspace {
    pub focus_handle: FocusHandle,
    /// Focus moves here while a modal is up, so the workspace's key bindings
    /// stop resolving — the keyboard half of making a modal modal.
    pub modal_focus: FocusHandle,
    pub title_bar: TitleBarState,
    pub settings: Settings,
    pub recents: Recents,
    pub journal: Journal,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub tool: Tool,
    pub modal: Option<Modal>,
    /// The toolbar colour dropdown.
    pub color_dropdown_open: bool,
    pub status: SharedString,
    /// What has been typed into the toolbar's page box, while it is being
    /// typed. `None` means the box is showing the current page instead.
    pub page_input: Option<String>,
    /// Focus moves here while the page box is being typed into, for the same
    /// reason it moves to `modal_focus` for a dialog: so the workspace's key
    /// bindings stop swallowing the digits.
    pub page_focus: FocusHandle,
    /// A scrollbar drag in progress, as the distance from the top of the thumb
    /// to where it was grabbed. Keeping the grab point rather than the pointer
    /// position is what stops the thumb jumping under the cursor on the first
    /// move.
    pub scroll_drag: Option<f32>,
    /// A selection drag that has reached the edge of the viewport and is
    /// pulling the page column along behind it.
    pub drag_scroll: Option<DragScroll>,
    /// The right-click menu, while it is open.
    pub context_menu: Option<ContextMenu>,
    /// An eraser stroke in progress.
    pub erasing: Option<EraseDrag>,
    /// When the last pinch (or Ctrl+wheel) step landed. While it is recent
    /// the viewer stretches the bitmaps it has instead of re-rendering them
    /// on every step.
    pub pinched: Option<Instant>,
    /// Set once the close flow is resolved, so the window actually closes
    /// instead of asking again.
    pub closing: bool,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();

        let mut workspace = Self {
            focus_handle: cx.focus_handle(),
            modal_focus: cx.focus_handle(),
            title_bar: TitleBarState::new(),
            recents: Recents::load(),
            journal: Journal::default(),
            tabs: Vec::new(),
            active: 0,
            tool: Tool::Select,
            modal: None,
            color_dropdown_open: false,
            status: "Ready".into(),
            page_input: None,
            page_focus: cx.focus_handle(),
            scroll_drag: None,
            drag_scroll: None,
            context_menu: None,
            erasing: None,
            pinched: None,
            closing: false,
            settings,
        };

        workspace.handle_previous_session();
        workspace
    }

    // --- theme -----------------------------------------------------------

    pub fn theme(&self) -> ThemeMode {
        if self.settings.dark_theme {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        }
    }

    pub fn palette(&self) -> Palette {
        Palette::for_mode(self.theme())
    }

    pub fn toggle_theme(&mut self) {
        self.settings.dark_theme = !self.settings.dark_theme;
        self.settings.save();
        self.status = format!("{} theme", self.theme().label()).into();
    }

    // --- tabs ------------------------------------------------------------

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    pub fn window_title(&self) -> String {
        match self.active_tab() {
            Some(tab) => format!(
                "{}{} — RustedPdf",
                tab.title(),
                if tab.is_dirty() { " •" } else { "" }
            ),
            None => "RustedPdf".to_string(),
        }
    }

    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    pub fn cycle_tab(&mut self, direction: i32) {
        if self.tabs.len() < 2 {
            return;
        }
        let count = self.tabs.len() as i32;
        let next = (self.active as i32 + direction).rem_euclid(count);
        self.active = next as usize;
    }

    // --- opening ---------------------------------------------------------

    /// The file picker, restricted to PDFs. Several files can be picked at
    /// once; each becomes a tab.
    pub fn open_with_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = rfd::FileDialog::new()
            .set_title("Open PDF")
            .add_filter("PDF documents", &["pdf"])
            .pick_files();

        let Some(paths) = picked else {
            return;
        };

        for path in paths {
            self.open_path(&path, window, cx);
        }
    }

    pub fn open_path(&mut self, path: &Path, _window: &mut Window, cx: &mut Context<Self>) {
        // Already open? Just go there.
        if let Some(index) = self.tabs.iter().position(|tab| tab.document.path == path) {
            self.active = index;
            self.status = format!("{} is already open", file_name(path)).into();
            cx.notify();
            return;
        }

        // The recent list can outlive the files it points at.
        if !path.is_file() {
            self.recents.remove(path);
            self.show_message(
                "File not found",
                format!(
                    "{} no longer exists, so it has been removed from Recent Files.",
                    path.display()
                ),
            );
            cx.notify();
            return;
        }

        if path
            .extension()
            .is_none_or(|extension| !extension.to_string_lossy().eq_ignore_ascii_case("pdf"))
        {
            self.show_message(
                "Not a PDF",
                format!(
                    "RustedPdf only opens PDF files. {} isn't one.",
                    file_name(path)
                ),
            );
            cx.notify();
            return;
        }

        match Document::open(path) {
            Ok(document) => {
                let pages = document.page_count();
                self.recents.record(path, pages);
                self.tabs
                    .push(Tab::new(document, self.settings.default_zoom));
                self.active = self.tabs.len() - 1;
                self.status = format!("Opened {} ({pages} pages)", file_name(path)).into();
            }
            Err(error) => self.show_message("Could not open document", error.to_string()),
        }

        cx.notify();
    }

    // --- marks -----------------------------------------------------------

    pub fn mark_kind(&self) -> Option<MarkKind> {
        match self.tool {
            Tool::Highlight => Some(MarkKind::Highlight),
            Tool::Underline => Some(MarkKind::Underline),
            _ => None,
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.end_erase();
        // Abandon a half-finished drag, but keep any completed selection —
        // switching tools shouldn't lose text you were about to copy.
        if let Some(tab) = self.active_tab_mut() {
            tab.drag = None;
            tab.preview.clear();
        }
        self.status = format!("{} tool", tool.label()).into();
    }

    pub fn set_mark_color(&mut self, color: u32) {
        self.settings.mark_color = color;
        self.settings.save();
        self.status = format!("Mark colour #{color:06x}").into();
    }

    /// How strongly new marks are applied. Kept out of `Mark` defaults so an
    /// existing mark's strength is never changed retroactively.
    pub fn set_mark_opacity(&mut self, opacity: f32) {
        self.settings.mark_opacity = opacity.clamp(0.05, 1.0);
        self.settings.save();
        self.status = format!(
            "Mark strength {}%",
            (self.settings.mark_opacity * 100.).round()
        )
        .into();
    }

    /// Copies the current text selection. Selecting is done with the Select
    /// tool: drag across text, then Ctrl+C.
    ///
    /// The selection is dropped once it's on the clipboard. Copying is the end
    /// of what a selection was for, and leaving the highlight sitting there
    /// afterwards reads as though the copy hadn't happened — every text field
    /// and reader clears it, so this does too.
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let text = self
            .active_tab()
            .and_then(|tab| tab.selection.as_ref())
            .map(|selection| selection.text.clone())
            .unwrap_or_default();

        if text.trim().is_empty() {
            self.status = "Nothing selected — drag across text with the Select tool".into();
            return;
        }

        let characters = text.chars().count();
        cx.write_to_clipboard(ClipboardItem::new_string(text));

        if let Some(tab) = self.active_tab_mut() {
            tab.clear_selection();
        }

        self.status = format!("Copied {characters} characters").into();
    }

    /// Selects all the text in the active document, whichever tool is in
    /// hand — selecting is never marking, so this is safe with the
    /// highlighter out too.
    pub fn select_all(&mut self) {
        let Some(tab) = self.active_tab_mut() else {
            self.status = "Open a PDF first".into();
            return;
        };

        self.status = match tab.select_all() {
            Some(characters) => {
                format!("Selected all {characters} characters — Ctrl+C to copy").into()
            }
            None => "This document has no selectable text".into(),
        };
    }

    fn has_selection(&self) -> bool {
        self.active_tab()
            .and_then(|tab| tab.selection.as_ref())
            .is_some_and(|selection| !selection.spans.is_empty())
    }

    /// Highlights or underlines the current selection — the context menu's
    /// way of marking text without switching tools.
    pub fn mark_selection(&mut self, kind: MarkKind) {
        let (color, opacity) = (self.settings.mark_color, self.settings.mark_opacity);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let Some(selection) = tab.selection.as_mut() else {
            self.status = "Nothing selected — drag across some text first".into();
            return;
        };

        let added: usize = selection
            .spans
            .iter()
            .map(|span| {
                tab.document
                    .add_marks(span.page, &span.areas, kind, color, opacity)
            })
            .sum();
        // Still selected, so Ctrl+C works, but no longer painted over the
        // colour just applied.
        selection.shown = false;

        self.status = match added {
            0 => "Nothing to mark in the selection".into(),
            1 => format!("{} added", kind.label()).into(),
            count => format!("{} added over {count} lines", kind.label()).into(),
        };
        self.record_journal();
    }

    /// Takes highlights and underlines off the selected text only.
    pub fn clear_marks_in_selection(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let Some(selection) = tab.selection.as_ref() else {
            self.status = "Nothing selected — drag across some text first".into();
            return;
        };

        let removed: usize = selection
            .spans
            .iter()
            .map(|span| tab.document.clear_marks_in(span.page, &span.areas))
            .sum();

        self.status = match removed {
            0 => "No marks in the selection".into(),
            1 => "Removed the mark from the selection".into(),
            count => format!("Removed {count} marks from the selection").into(),
        };
        if removed > 0 {
            self.record_journal();
        }
    }

    /// Removes the marks under where the context menu was opened.
    pub fn remove_mark_here(&mut self) {
        let Some((page, (x, y))) = self.context_menu.and_then(|menu| menu.target) else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };

        let removed = tab.document.remove_marks_at(page, x, y);
        self.status = match removed {
            0 => "No mark there".into(),
            1 => "Removed a mark".into(),
            count => format!("Removed {count} marks").into(),
        };
        if removed > 0 {
            self.record_journal();
        }
    }

    /// Whether there is a mark under where the context menu was opened.
    pub fn context_menu_has_mark(&self) -> bool {
        let Some((page, (x, y))) = self.context_menu.and_then(|menu| menu.target) else {
            return false;
        };
        self.active_tab().is_some_and(|tab| {
            tab.document
                .marks
                .iter()
                .any(|mark| mark.page == page && mark.hit_area().contains(x, y))
        })
    }

    pub fn context_menu_has_selection(&self) -> bool {
        self.has_selection()
    }

    /// Opens the right-click menu at `position`.
    pub fn open_context_menu(&mut self, position: Point<Pixels>) {
        self.title_bar.menu.close();
        self.color_dropdown_open = false;
        self.close_page_input();

        let target = self.active_tab().and_then(|tab| tab.page_at(position));
        self.context_menu = Some(ContextMenu { position, target });
    }

    pub fn close_context_menu(&mut self) -> bool {
        self.context_menu.take().is_some()
    }

    // --- eraser strokes ----------------------------------------------------

    /// Starts an eraser stroke at `position`, removing whatever is under it.
    pub fn begin_erase(&mut self, position: Point<Pixels>) {
        let removed = self.erase_at(position);
        self.erasing = Some(EraseDrag {
            last: position,
            removed,
        });
        self.status = if removed > 0 {
            "Erasing — drag across marks to remove them".into()
        } else {
            "No mark under the pointer — drag across marks to remove them".into()
        };
    }

    /// Carries the stroke on to `position`. Returns whether anything was
    /// removed, so the caller knows to repaint.
    pub fn continue_erase(&mut self, position: Point<Pixels>) -> bool {
        let Some(stroke) = self.erasing else {
            return false;
        };

        // Sample the segment every few pixels — finer than the thinnest
        // underline's hit area.
        const STEP: f32 = 3.;
        let dx = f32::from(position.x - stroke.last.x);
        let dy = f32::from(position.y - stroke.last.y);
        let steps = ((dx.hypot(dy) / STEP).ceil() as usize).max(1);

        let mut removed = 0;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            removed += self.erase_at(Point {
                x: stroke.last.x + px(dx * t),
                y: stroke.last.y + px(dy * t),
            });
        }

        self.erasing = Some(EraseDrag {
            last: position,
            removed: stroke.removed + removed,
        });
        if removed > 0 {
            self.status = format!(
                "Removed {} so far",
                plural(stroke.removed + removed, "mark")
            )
            .into();
        }
        removed > 0
    }

    /// Ends the stroke. Returns whether one was in progress.
    pub fn end_erase(&mut self) -> bool {
        let Some(stroke) = self.erasing.take() else {
            return false;
        };

        self.status = match stroke.removed {
            0 => "No mark under the pointer".into(),
            count => format!("Removed {}", plural(count, "mark")).into(),
        };
        // Once per stroke, not once per mark: the journal is written to disk.
        if stroke.removed > 0 {
            self.record_journal();
        }
        true
    }

    fn erase_at(&mut self, position: Point<Pixels>) -> usize {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return 0;
        };
        match tab.page_at(position) {
            Some((page, (x, y))) => tab.document.remove_marks_at(page, x, y),
            None => 0,
        }
    }

    pub fn undo_mark(&mut self) {
        let removed = self
            .active_tab_mut()
            .map(|tab| tab.document.undo_last_mark())
            .unwrap_or(false);

        self.status = if removed {
            "Removed the last mark".into()
        } else {
            "Nothing to undo".into()
        };
        self.record_journal();
    }

    pub fn clear_marks(&mut self, page_only: bool) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };

        let removed = if page_only {
            let page = tab.current_page();
            tab.document.clear_marks_on_page(page)
        } else {
            tab.document.clear_all_marks()
        };

        self.status = match removed {
            0 => "No marks to clear".into(),
            1 => "Cleared 1 mark".into(),
            count => format!("Cleared {count} marks").into(),
        };
        self.record_journal();
    }

    /// Mirrors the active document's unsaved marks into the crash journal.
    /// Called after every change that touches marks.
    pub fn record_journal(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let (path, marks, dirty) = (
            tab.document.path.clone(),
            tab.document.marks.clone(),
            tab.document.dirty,
        );
        self.journal.record(&path, &marks, dirty);
    }

    // --- page navigation --------------------------------------------------

    /// Scrolls the active document to `page` (zero-based), clamped to what the
    /// document actually has.
    pub fn go_to_page(&mut self, page: usize) {
        let Some(tab) = self.tabs.get(self.active) else {
            self.status = "Open a PDF first".into();
            return;
        };

        let count = tab.document.page_count();
        let page = page.min(count.saturating_sub(1));
        tab.scroll_to_page(page);

        self.status = format!("Page {} of {count}", page + 1).into();
    }

    /// Moves one page forward or back, from wherever the viewport is now.
    pub fn step_page(&mut self, direction: i32) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };

        let last = tab.document.page_count().saturating_sub(1) as i32;
        let next = (tab.current_page() as i32 + direction).clamp(0, last);
        self.go_to_page(next as usize);
    }

    /// Starts editing the toolbar's page box, pre-filled with where you are —
    /// so typing over it is a jump and pressing Enter straight away isn't.
    pub fn open_page_input(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else {
            self.status = "Open a PDF first".into();
            return;
        };

        self.page_input = Some((tab.current_page() + 1).to_string());
        self.title_bar.menu.close();
        self.color_dropdown_open = false;
        self.status = "Type a page number, then Enter".into();
    }

    pub fn close_page_input(&mut self) -> bool {
        self.page_input.take().is_some()
    }

    /// Feeds one keystroke into the page box. Returns whether it was used, so
    /// the caller knows whether to stop the key going any further.
    ///
    /// Only digits, Backspace, Enter and Escape mean anything here. Everything
    /// else is left alone rather than swallowed, because the box holds a
    /// number and nothing else ever belongs in it.
    pub fn edit_page_input(&mut self, key: &str) -> bool {
        if self.page_input.is_none() {
            return false;
        }

        match key {
            "escape" => {
                self.page_input = None;
                self.status = "Left the page unchanged".into();
            }
            "enter" => {
                let entered = self.page_input.take().unwrap_or_default();
                match entered.trim().parse::<usize>() {
                    Ok(number) if number >= 1 => self.go_to_page(number - 1),
                    _ => {
                        self.status = if entered.trim().is_empty() {
                            "No page number typed".into()
                        } else {
                            format!("{entered} isn't a page number").into()
                        }
                    }
                }
            }
            "backspace" => {
                if let Some(text) = self.page_input.as_mut() {
                    text.pop();
                }
            }
            digit if digit.len() == 1 && digit.starts_with(|c: char| c.is_ascii_digit()) => {
                if let Some(text) = self.page_input.as_mut() {
                    // Longer than any document has pages: this is a held key,
                    // not someone typing.
                    if text.len() < 6 {
                        text.push_str(digit);
                    }
                }
            }
            _ => return false,
        }

        true
    }

    // --- selection drags ---------------------------------------------------

    /// Extends the drag in progress to `pointer`, and decides whether the
    /// viewport should start creeping after it.
    ///
    /// Returns whether anything changed.
    pub fn continue_text_drag(&mut self, pointer: Point<Pixels>) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        if tab.drag.is_none() {
            self.drag_scroll = None;
            return false;
        }

        // Wherever the pointer has got to — another page, the gap between
        // two, off the viewport — the selection runs to the nearest place in
        // the document rather than stopping at the edge of where it began.
        let extended = tab.extend_drag(tab.drag_target(pointer));

        let bounds = tab.scroll.bounds();
        let creeping = autoscroll_velocity(
            f32::from(pointer.y),
            f32::from(bounds.origin.y),
            f32::from(bounds.origin.y + bounds.size.height),
        ) != 0.;

        if !creeping {
            self.drag_scroll = None;
            return extended;
        }

        // The clock carries over from the step before. Restarting it on every
        // move would mean a drag that jiggles never accumulates enough elapsed
        // time to scroll at all.
        let stepped = self
            .drag_scroll
            .map_or_else(Instant::now, |scroll| scroll.stepped);
        self.drag_scroll = Some(DragScroll { pointer, stepped });

        true
    }

    /// Applies one auto-scroll step, sized by how long since the last one.
    /// Returns whether the viewport is still creeping and wants another frame.
    pub fn step_drag_scroll(&mut self) -> bool {
        let Some(scroll) = self.drag_scroll else {
            return false;
        };
        let Some(tab) = self.tabs.get_mut(self.active) else {
            self.drag_scroll = None;
            return false;
        };
        if tab.drag.is_none() {
            self.drag_scroll = None;
            return false;
        }

        let bounds = tab.scroll.bounds();
        let velocity = autoscroll_velocity(
            f32::from(scroll.pointer.y),
            f32::from(bounds.origin.y),
            f32::from(bounds.origin.y + bounds.size.height),
        );
        if velocity == 0. {
            self.drag_scroll = None;
            return false;
        }

        // A rate, not a per-frame nudge, so the speed is the same on a 60Hz
        // panel and a 144Hz one. The cap keeps a stalled frame — a page
        // rasterising, the window coming back from behind another — from
        // launching the viewport across the document in one jump.
        let now = Instant::now();
        let elapsed = now.duration_since(scroll.stepped).as_secs_f32().min(0.05);

        tab.scroll_to_offset(tab.scroll_offset() + velocity * elapsed);
        let offset = tab.scroll_offset();
        let furthest = tab.max_scroll_offset();

        // The content just moved under a stationary pointer, so the selection
        // has to be re-read at the same place on screen.
        tab.extend_drag(tab.drag_target(scroll.pointer));

        self.drag_scroll = Some(DragScroll {
            pointer: scroll.pointer,
            stepped: now,
        });

        // Pinned against an end of the document there is nothing left to creep
        // towards, so stop asking for frames. Dragging back the other way arms
        // it again on the next move.
        //
        // This asks where the viewport *is*, not whether it moved: a frame
        // that arrives a fraction of a millisecond after the last one moves
        // less than a pixel, and reading that as "stuck" would stall a drag
        // being held perfectly still — which is the way this gets used.
        let pinned = (velocity < 0. && offset <= 0.01)
            || (velocity > 0. && furthest > 0. && offset >= furthest - 0.01);
        if pinned {
            self.drag_scroll = None;
            return false;
        }

        true
    }

    // --- scrollbar ---------------------------------------------------------

    /// Picks the thumb up. A press on the track rather than the thumb takes it
    /// by the middle, so the part of the document you pointed at comes to the
    /// pointer instead of the thumb jumping a screen away from it.
    pub fn begin_scroll_drag(&mut self, pointer_y: f32) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some(bar) = tab.scrollbar() else { return };

        let local = pointer_y - bar.window_top;
        let on_thumb = local >= bar.thumb_top && local <= bar.thumb_top + bar.thumb;
        let grab = if on_thumb {
            local - bar.thumb_top
        } else {
            bar.thumb / 2.
        };

        self.scroll_drag = Some(grab);
        tab.scroll_to_thumb(local - grab);
    }

    /// Continues a scrollbar drag. Returns whether one was in progress.
    pub fn continue_scroll_drag(&mut self, pointer_y: f32) -> bool {
        let Some(grab) = self.scroll_drag else {
            return false;
        };
        let Some(tab) = self.tabs.get(self.active) else {
            return false;
        };
        let Some(bar) = tab.scrollbar() else {
            return false;
        };

        tab.scroll_to_thumb(pointer_y - bar.window_top - grab);
        true
    }

    pub fn end_scroll_drag(&mut self) -> bool {
        self.scroll_drag.take().is_some()
    }

    // --- zoom ------------------------------------------------------------

    pub fn zoom_step(&mut self, direction: i32) {
        if let Some(tab) = self.active_tab_mut() {
            tab.zoom_step(direction);
            let zoom = tab.zoom;
            self.status = format!("Zoom {}%", (zoom * 100.).round()).into();
        }
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        if let Some(tab) = self.active_tab_mut() {
            tab.set_zoom(zoom);
            let zoom = tab.zoom;
            self.status = format!("Zoom {}%", (zoom * 100.).round()).into();
        }
    }

    /// One step of a pinch, or of the wheel with Ctrl held — both arrive as
    /// the same event on Windows. `amount` is the scroll distance in pixels,
    /// positive for zooming in, and the zoom is about `anchor`.
    ///
    /// The zoom is continuous rather than snapping to the toolbar's stops: a
    /// pinch is a stream of small steps, and snapping each one either goes
    /// nowhere or lurches a quarter at a time.
    pub fn pinch_zoom(&mut self, amount: f32, anchor: Point<Pixels>) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };

        // A notch of an ordinary wheel is about 60px here, which this makes
        // a 16% step — one toolbar stop or so.
        let zoom = tab.zoom * (amount * 0.0025).exp();
        if !tab.zoom_at(zoom, anchor) {
            return false;
        }

        let zoom = tab.zoom;
        self.pinched = Some(Instant::now());
        self.status = format!("Zoom {}%", (zoom * 100.).round()).into();
        true
    }

    /// Whether a pinch is still in progress, so pages may be stretched rather
    /// than re-rendered.
    pub fn is_pinching(&self) -> bool {
        self.pinched
            .is_some_and(|pinched| pinched.elapsed() < PINCH_SETTLE)
    }

    pub fn zoom_percent(&self) -> u32 {
        self.active_tab()
            .map(|tab| (tab.zoom * 100.).round() as u32)
            .unwrap_or_else(|| (self.settings.default_zoom * 100.).round() as u32)
    }

    // --- saving ----------------------------------------------------------

    pub fn save_tab(&mut self, index: usize) -> bool {
        let Some(tab) = self.tabs.get_mut(index) else {
            return false;
        };
        if !tab.document.dirty {
            return true;
        }

        match tab.document.save() {
            Ok(()) => {
                let path = tab.document.path.clone();
                self.journal.forget(&path);
                self.status = format!("Saved {}", file_name(&path)).into();
                true
            }
            Err(error) => {
                self.show_message("Could not save", error.to_string());
                false
            }
        }
    }

    pub fn save_active(&mut self) {
        self.save_tab(self.active);
    }

    pub fn save_active_as(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };

        let picked = rfd::FileDialog::new()
            .set_title("Save a copy")
            .add_filter("PDF documents", &["pdf"])
            .set_file_name(tab.document.file_name())
            .save_file();

        let Some(path) = picked else { return };
        let path = with_pdf_extension(path);

        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        match tab.document.save_as(&path) {
            Ok(()) => {
                let pages = tab.document.page_count();
                self.recents.record(&path, pages);
                self.status = format!("Saved as {}", file_name(&path)).into();
                self.record_journal();
            }
            Err(error) => self.show_message("Could not save", error.to_string()),
        }
    }

    pub fn save_all(&mut self) -> bool {
        let mut all_saved = true;
        for index in 0..self.tabs.len() {
            if !self.save_tab(index) {
                all_saved = false;
            }
        }
        if all_saved {
            self.status = "Saved all documents".into();
        }
        all_saved
    }

    // --- closing ---------------------------------------------------------

    fn unsaved_targets(&self, scope: CloseScope) -> Vec<CloseTarget> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(index, tab)| {
                tab.is_dirty()
                    && match scope {
                        CloseScope::Window => true,
                        CloseScope::Tab(only) => *index == only,
                    }
            })
            .map(|(index, tab)| CloseTarget {
                tab: index,
                name: tab.title(),
                marks: tab.document.marks.len(),
                selected: true,
            })
            .collect()
    }

    /// Returns true when the window may close now; otherwise a dialog is up and
    /// the close will be finished from there.
    pub fn request_close_window(&mut self) -> bool {
        if self.closing {
            return true;
        }

        let targets = self.unsaved_targets(CloseScope::Window);
        if targets.is_empty() || !self.settings.confirm_on_close {
            self.finish_close();
            return true;
        }

        self.modal = Some(Modal::ConfirmClose {
            scope: CloseScope::Window,
            targets,
        });
        false
    }

    pub fn request_close_tab(&mut self, index: usize) {
        if self.tabs.get(index).is_none_or(|tab| !tab.is_dirty()) || !self.settings.confirm_on_close
        {
            self.close_tab(index);
            return;
        }

        self.modal = Some(Modal::ConfirmClose {
            scope: CloseScope::Tab(index),
            targets: self.unsaved_targets(CloseScope::Tab(index)),
        });
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }

        let tab = self.tabs.remove(index);
        self.journal.forget(&tab.document.path);
        self.status = format!("Closed {}", tab.title()).into();

        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    /// Everything is resolved: no unsaved work is being abandoned silently, so
    /// the journal has nothing to recover.
    pub fn finish_close(&mut self) {
        self.journal.discard();
        self.closing = true;
    }

    /// Applies a choice from the save-before-closing dialog.
    pub fn resolve_close(
        &mut self,
        scope: CloseScope,
        save: CloseChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected: Vec<usize> = match (&self.modal, save) {
            (Some(Modal::ConfirmClose { targets, .. }), CloseChoice::SaveSelected) => targets
                .iter()
                .filter(|target| target.selected)
                .map(|target| target.tab)
                .collect(),
            (Some(Modal::ConfirmClose { targets, .. }), CloseChoice::SaveAll) => {
                targets.iter().map(|target| target.tab).collect()
            }
            _ => Vec::new(),
        };

        self.modal = None;

        let mut saved_everything = true;
        for index in selected {
            if !self.save_tab(index) {
                saved_everything = false;
            }
        }

        // A failed save (permissions, a locked file) must not take the
        // document down with it.
        if !saved_everything {
            return;
        }

        match scope {
            CloseScope::Window => {
                self.finish_close();
                window.remove_window();
            }
            CloseScope::Tab(index) => self.close_tab(index),
        }

        cx.notify();
    }

    /// Adds or removes RustedPdf from the OS "Open with" menu for PDFs.
    pub fn set_open_with(&mut self, enabled: bool) {
        let result = if enabled {
            integration::register()
        } else {
            integration::unregister()
        };

        match result {
            Ok(()) if enabled => self.status = "RustedPdf now appears in Open with for PDFs".into(),
            Ok(()) => self.status = "Removed RustedPdf from Open with".into(),
            Err(error) => {
                self.show_message("Could not change the file association", error.to_string())
            }
        }
    }

    // --- exporting -------------------------------------------------------

    /// Opens the export dialog, with the page you're on ticked to start with.
    pub fn open_export(&mut self) {
        let Some(tab) = self.active_tab() else {
            self.status = "Open a PDF first".into();
            return;
        };

        let current = tab.current_page();
        let pages = (0..tab.document.page_count())
            .map(|index| index == current)
            .collect();

        self.open_modal(Modal::Export {
            pages,
            format: ExportFormat::Png,
        });
    }

    pub fn set_export_page(&mut self, index: usize, selected: bool) {
        if let Some(Modal::Export { pages, .. }) = self.modal.as_mut() {
            if let Some(flag) = pages.get_mut(index) {
                *flag = selected;
            }
        }
    }

    /// Ticks or unticks every page at once.
    pub fn set_all_export_pages(&mut self, selected: bool) {
        if let Some(Modal::Export { pages, .. }) = self.modal.as_mut() {
            pages.iter_mut().for_each(|flag| *flag = selected);
        }
    }

    pub fn set_export_format(&mut self, format: ExportFormat) {
        if let Some(Modal::Export {
            format: current, ..
        }) = self.modal.as_mut()
        {
            *current = format;
        }
    }

    /// Asks for a destination and writes the chosen pages out.
    pub fn run_export(&mut self) {
        let Some(Modal::Export { pages, format }) = self.modal.as_ref() else {
            return;
        };
        let format = *format;
        let selected: Vec<usize> = pages
            .iter()
            .enumerate()
            .filter(|(_, chosen)| **chosen)
            .map(|(index, _)| index)
            .collect();

        if selected.is_empty() {
            self.status = "Tick at least one page to export".into();
            return;
        }

        let Some(tab) = self.active_tab() else { return };
        let extension = format.extension(selected.len());
        let suggested = format!(
            "{}-pages.{extension}",
            tab.document
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
        );

        // The dialog is the point of no return, so close ours first — two
        // modal windows at once is never what anyone wants.
        self.modal = None;

        let picked = rfd::FileDialog::new()
            .set_title("Export pages")
            .add_filter(format.label(), &[extension])
            .set_file_name(suggested)
            .save_file();

        let Some(destination) = picked else {
            self.status = "Export cancelled".into();
            return;
        };
        let destination = export::with_extension(destination, extension);

        let result = match format {
            ExportFormat::Pdf => self
                .active_tab()
                .map(|tab| tab.document.export_pages_as_pdf(&selected, &destination))
                .unwrap_or_else(|| Ok(())),
            // Rendered inside the writer, a page at a time: a whole-book export
            // holds one page bitmap rather than one per page.
            ExportFormat::Png => match self.tabs.get_mut(self.active) {
                Some(tab) => export::write_images(&selected, &destination, |index| {
                    tab.document
                        .render_page_for_export(index, export::EXPORT_SCALE)
                }),
                None => Ok(()),
            },
        };

        match result {
            Ok(()) => {
                self.status = format!(
                    "Exported {} to {}",
                    format.describe(selected.len()),
                    file_name(&destination)
                )
                .into()
            }
            Err(error) => self.show_message("Export failed", error.to_string()),
        }
    }

    // --- modals ----------------------------------------------------------

    pub fn show_message(&mut self, title: impl Into<SharedString>, body: impl Into<SharedString>) {
        self.modal = Some(Modal::Message {
            title: title.into(),
            body: body.into(),
        });
    }

    pub fn open_modal(&mut self, modal: Modal) {
        self.title_bar.menu.close();
        self.color_dropdown_open = false;
        self.page_input = None;
        self.modal = Some(modal);
    }

    pub fn dismiss_modal(&mut self) -> bool {
        self.modal.take().is_some()
    }

    pub fn modal_is_open(&self) -> bool {
        self.modal.is_some()
    }

    /// Toggles one row of the save-before-closing dialog.
    pub fn toggle_close_target(&mut self, tab: usize) {
        if let Some(Modal::ConfirmClose { targets, .. }) = self.modal.as_mut() {
            if let Some(target) = targets.iter_mut().find(|target| target.tab == tab) {
                target.selected = !target.selected;
            }
        }
    }

    // --- previous session -------------------------------------------------

    /// Applies the crash-recovery policy to whatever the last session left
    /// behind. Runs before the window is shown, so any notice is the first
    /// thing the user sees.
    fn handle_previous_session(&mut self) {
        let Some(journal) = Journal::take_previous() else {
            return;
        };

        if !self.settings.recover_after_crash {
            let count = journal.entries.len();
            self.show_message(
                "Unsaved marks discarded",
                format!(
                    "The last session ended unexpectedly with unsaved marks in {count} \
                     document{}. They were discarded and the files on disk are unchanged.\n\n\
                     To restore them next time, turn on \"Recover unsaved marks after a crash\" \
                     in Settings.",
                    if count == 1 { "" } else { "s" }
                ),
            );
            return;
        }

        let mut recovered = Vec::new();
        let mut failed = Vec::new();

        for entry in journal.entries {
            match Document::open(&entry.path) {
                Ok(mut document) => {
                    document.restore_marks(entry.marks);
                    recovered.push(document.file_name());
                    self.tabs
                        .push(Tab::new(document, self.settings.default_zoom));
                }
                Err(error) => failed.push(format!("{}: {error}", file_name(&entry.path))),
            }
        }

        if recovered.is_empty() && failed.is_empty() {
            return;
        }

        let mut body = String::new();
        if !recovered.is_empty() {
            body.push_str(&format!(
                "Restored unsaved marks in:\n{}\n\nThey are not saved yet — use File ▸ Save.",
                recovered
                    .iter()
                    .map(|name| format!("  • {name}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !failed.is_empty() {
            body.push_str(&format!("\n\nCould not reopen:\n  {}", failed.join("\n  ")));
        }

        self.show_message("Recovered from the last session", body);

        // The restored marks are unsaved, so they belong in the new journal.
        self.record_journal();
    }

    // --- command dispatch -------------------------------------------------

    /// Runs a command by id. Menu items and key bindings both come through
    /// here, so there is one place where behaviour is defined.
    pub fn run_command(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "file.open" => self.open_with_dialog(window, cx),
            "file.export" => self.open_export(),
            "file.save" => self.save_active(),
            "file.save_as" => self.save_active_as(),
            "file.save_all" => {
                self.save_all();
            }
            "file.close_tab" => self.request_close_tab(self.active),
            "file.exit" => {
                if self.request_close_window() {
                    window.remove_window();
                }
            }

            "edit.copy" => self.copy_selection(cx),
            "edit.select_all" => self.select_all(),
            "edit.highlight_selection" => self.mark_selection(MarkKind::Highlight),
            "edit.underline_selection" => self.mark_selection(MarkKind::Underline),
            "edit.clear_selection_marks" => self.clear_marks_in_selection(),
            "edit.remove_mark_here" => self.remove_mark_here(),
            "edit.undo_annotation" => self.undo_mark(),
            "edit.clear_page" => self.clear_marks(true),
            "edit.clear_document" => self.clear_marks(false),
            "edit.settings" => self.open_modal(Modal::Settings),

            "view.zoom_in" => self.zoom_step(1),
            "view.zoom_out" => self.zoom_step(-1),
            "view.zoom_reset" => self.set_zoom(1.0),
            "view.zoom.50" => self.set_zoom(0.5),
            "view.zoom.75" => self.set_zoom(0.75),
            "view.zoom.100" => self.set_zoom(1.0),
            "view.zoom.150" => self.set_zoom(1.5),
            "view.zoom.200" => self.set_zoom(2.0),
            "view.theme" => self.toggle_theme(),
            "view.go_to_page" => self.open_page_input(),
            "view.first_page" => self.go_to_page(0),
            "view.last_page" => {
                let last = self
                    .active_tab()
                    .map(|tab| tab.document.page_count().saturating_sub(1))
                    .unwrap_or(0);
                self.go_to_page(last);
            }

            "tools.select" => self.set_tool(Tool::Select),
            "tools.highlight" => self.set_tool(Tool::Highlight),
            "tools.underline" => self.set_tool(Tool::Underline),
            "tools.erase" => self.set_tool(Tool::Erase),
            "tools.color" => self.color_dropdown_open = !self.color_dropdown_open,

            "help.shortcuts" => self.open_modal(Modal::Shortcuts),
            "help.about" => self.open_modal(Modal::About),

            "file.reveal" => {
                if let Some(tab) = self.active_tab() {
                    cx.reveal_path(&tab.document.path);
                    self.status = format!("Showing {} in the file manager", tab.title()).into();
                }
            }

            other => self.status = format!("{other} isn't implemented yet").into(),
        }

        // Whatever was picked, a context menu it was picked from is done.
        self.context_menu = None;
        cx.notify();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    SaveSelected,
    SaveAll,
    Discard,
}

impl TitleBarHost for Workspace {
    fn title_bar(&mut self) -> &mut TitleBarState {
        &mut self.title_bar
    }

    fn on_menu_command(
        &mut self,
        id: &'static str,
        _label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_command(id, window, cx);
    }

    fn on_close_requested(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let may_close = self.request_close_window();
        cx.notify();
        may_close
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// "1 mark", "3 marks".
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn with_pdf_extension(path: PathBuf) -> PathBuf {
    match path.extension() {
        Some(extension) if extension.to_string_lossy().eq_ignore_ascii_case("pdf") => path,
        _ => path.with_extension("pdf"),
    }
}

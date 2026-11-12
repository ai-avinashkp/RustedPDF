//! A reusable custom titlebar for GPUI apps: app mark, navigation menu with
//! nested submenus, window title, and the minimize / maximize / close buttons —
//! all on one horizontal strip.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │ [R] File Edit View Tools Help │   Untitled — RustedPdf   │ ─  ▢  ✕  │
//! └──────────────────────────────────────────────────────────────────────┘
//!   mark   nav menu (clickable)      drag region (title)     window controls
//! ```
//!
//! # Using it in another app
//!
//! 1. Open the window with `TitlebarOptions { appears_transparent: true, .. }`
//!    (see [`window_options_titlebar`] for the exact options, including the
//!    macOS traffic-light placement).
//! 2. Give the root view a [`TitleBarState`] field and implement
//!    [`TitleBarHost`] for it.
//! 3. Render `TitleBar::new(title, MENUS).style(..).render(&self.title_bar, window, cx)`.
//!
//! ```ignore
//! impl TitleBarHost for Workspace {
//!     fn title_bar(&mut self) -> &mut TitleBarState { &mut self.title_bar }
//!
//!     fn on_menu_command(&mut self, id: &'static str, label: &'static str,
//!                        window: &mut Window, cx: &mut Context<Self>) {
//!         match id { "file.exit" => window.remove_window(), _ => {} }
//!     }
//! }
//! ```
//!
//! Everything that isn't clickable is a drag region: press-and-move starts a
//! native window move, double-click toggles maximize/restore. The window
//! buttons are ordinary client elements wired to explicit window calls — on
//! Windows they must *not* sit inside a caption region, or the OS swallows the
//! clicks before gpui sees them.

// Reusable component: parts of its API exist for host apps, not for this one.
#![allow(dead_code)]

pub mod menu;
pub mod style;
pub mod window_ops;

use gpui::{
    deferred, div, point, prelude::*, px, rgb, AnyElement, Context, Div, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, SharedString,
    TitlebarOptions, Window,
};

// Re-exported for host apps; not all of it is used inside this crate.
#[allow(unused_imports)]
pub use menu::{cmd, disabled, sub, toggle, Item, Menu, MenuState, SEP};
pub use style::{TitleBarMetrics, TitleBarStyle};

/// Paint layer for the dropdowns. A host that stacks its own floating layers
/// (modals, popovers) should keep them above or below this deliberately.
pub const MENU_LAYER: usize = 2;

/// State the titlebar needs the host to hold across frames.
#[derive(Debug, Default)]
pub struct TitleBarState {
    pub menu: MenuState,
    /// Where a press on the drag region started, until it becomes a drag.
    press: Option<Point<Pixels>>,
}

impl TitleBarState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Implemented by the view that owns the titlebar.
pub trait TitleBarHost: Render + Sized {
    fn title_bar(&mut self) -> &mut TitleBarState;

    /// Invoked when a menu command is clicked. The menu has already closed.
    fn on_menu_command(
        &mut self,
        id: &'static str,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Called when the close button is pressed. Return `false` to keep the
    /// window open — e.g. to put up a "save your changes?" prompt first, in
    /// which case the host is responsible for closing later.
    ///
    /// The default closes immediately.
    fn on_close_requested(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        true
    }
}

/// The window options this component expects: a transparent (client-side)
/// titlebar, with the macOS traffic lights centred in a bar of `height`.
pub fn window_options_titlebar(title: impl Into<SharedString>, height: Pixels) -> TitlebarOptions {
    TitlebarOptions {
        title: Some(title.into()),
        appears_transparent: true,
        traffic_light_position: if cfg!(target_os = "macos") {
            // Traffic lights are ~14px tall; centre them in our strip.
            Some(point(px(12.), (height - px(14.)) / 2.))
        } else {
            None
        },
    }
}

/// Builder for one frame of the titlebar.
pub struct TitleBar {
    title: SharedString,
    mark: Option<SharedString>,
    menus: &'static [Menu],
    style: TitleBarStyle,
    metrics: TitleBarMetrics,
    menus_enabled: bool,
    window_controls: bool,
    trailing: Vec<AnyElement>,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>, menus: &'static [Menu]) -> Self {
        Self {
            title: title.into(),
            mark: None,
            menus,
            style: TitleBarStyle::default(),
            metrics: TitleBarMetrics::default(),
            menus_enabled: true,
            // macOS keeps its native traffic lights.
            window_controls: !cfg!(target_os = "macos"),
            trailing: Vec::new(),
        }
    }

    /// A one or two character mark drawn in the accent square on the far left.
    pub fn mark(mut self, mark: impl Into<SharedString>) -> Self {
        self.mark = Some(mark.into());
        self
    }

    pub fn style(mut self, style: TitleBarStyle) -> Self {
        self.style = style;
        self
    }

    pub fn metrics(mut self, metrics: TitleBarMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Set to false to make the menus inert — e.g. while a modal is up.
    pub fn menus_enabled(mut self, enabled: bool) -> Self {
        self.menus_enabled = enabled;
        self
    }

    /// Adds an element between the title and the window buttons.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    pub fn height(&self) -> Pixels {
        self.metrics.height
    }

    // --- rendering -------------------------------------------------------

    pub fn render<V: TitleBarHost>(
        mut self,
        state: &TitleBarState,
        window: &Window,
        cx: &mut Context<V>,
    ) -> Div {
        let style = self.style;
        let metrics = self.metrics;
        let maximized = window.is_maximized();
        let trailing = std::mem::take(&mut self.trailing);

        let mut bar = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(metrics.height)
            .bg(rgb(style.background))
            .border_b_1()
            .border_color(rgb(style.border))
            .when(cfg!(target_os = "macos"), |bar| {
                bar.pl(metrics.traffic_lights_width)
            });

        if let Some(mark) = self.mark.clone() {
            bar = bar.child(
                self.drag_region::<V>(cx)
                    .px_2()
                    .child(self.app_mark(mark, style)),
            );
        }

        bar = bar
            .child(self.nav_menu::<V>(state, cx))
            // Filler: takes the remaining width and carries the window title.
            .child(
                self.drag_region::<V>(cx)
                    .flex_1()
                    .min_w(px(0.))
                    .justify_center()
                    .px_2()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_color(rgb(style.muted))
                            .whitespace_nowrap()
                            .child(self.title.clone()),
                    ),
            )
            .children(trailing);

        if self.window_controls {
            bar = bar.child(self.window_controls::<V>(maximized, cx));
        }

        bar
    }

    fn app_mark(&self, mark: SharedString, style: TitleBarStyle) -> Div {
        div()
            .w(px(20.))
            .h(px(20.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .bg(rgb(style.mark_background))
            .text_color(rgb(style.mark_text))
            .text_size(px(11.))
            .child(mark)
    }

    // --- navigation menu -------------------------------------------------

    fn nav_menu<V: TitleBarHost>(&self, state: &TitleBarState, cx: &mut Context<V>) -> Div {
        let mut nav = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(2.))
            .px_1()
            .h_full();

        for (index, menu) in self.menus.iter().enumerate() {
            nav = nav.child(self.menu_button::<V>(index, menu, state, cx));
        }
        nav
    }

    fn menu_button<V: TitleBarHost>(
        &self,
        index: usize,
        menu: &'static Menu,
        state: &TitleBarState,
        cx: &mut Context<V>,
    ) -> Div {
        let style = self.style;
        let metrics = self.metrics;
        let is_open = self.menus_enabled && state.menu.open == Some(index);
        let interactive = self.menus_enabled;

        div()
            .relative()
            .child(
                div()
                    .id(SharedString::from(format!("titlebar-nav-{index}")))
                    .h(metrics.nav_button_height)
                    .px_2()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .rounded_md()
                    .whitespace_nowrap()
                    .text_color(rgb(if interactive {
                        style.text
                    } else {
                        style.disabled
                    }))
                    .when(is_open, |b| b.bg(rgb(style.active)))
                    .when(interactive, |b| {
                        b.cursor_pointer()
                            .hover(move |s| s.bg(rgb(style.hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.title_bar().menu.toggle(index);
                                    cx.notify();
                                }),
                            )
                            // Once a menu is open, sliding across the bar
                            // switches to the one under the pointer.
                            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                                let menu = &mut this.title_bar().menu;
                                if *hovering && menu.is_open() && menu.open != Some(index) {
                                    menu.switch_to(index);
                                    cx.notify();
                                }
                            }))
                    })
                    .child(menu.label)
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(style.muted))
                            .child("▾"),
                    ),
            )
            .when(is_open, |wrapper| {
                wrapper.child(
                    deferred(
                        div()
                            .absolute()
                            .top(metrics.dropdown_top())
                            .left(px(0.))
                            .child(self.panel::<V>(index, menu.items, 0, state, cx)),
                    )
                    .with_priority(MENU_LAYER),
                )
            })
    }

    /// A menu panel. Recurses for whichever submenu is open at this `depth`, so
    /// nesting is limited only by the data.
    fn panel<V: TitleBarHost>(
        &self,
        menu_index: usize,
        items: &'static [Item],
        depth: usize,
        state: &TitleBarState,
        cx: &mut Context<V>,
    ) -> Div {
        let style = self.style;
        let metrics = self.metrics;

        let mut panel = div()
            .relative()
            .w(metrics.panel_width)
            .py(metrics.panel_padding)
            .rounded_md()
            .bg(rgb(style.panel))
            .border_1()
            .border_color(rgb(style.border))
            .shadow_lg()
            .text_color(rgb(style.text))
            // Keep clicks inside the menu away from the dismiss backdrop.
            .occlude();

        for (index, item) in items.iter().enumerate() {
            panel = panel.child(self.row::<V>(menu_index, item, index, depth, state, cx));
        }

        // The open submenu is added last, so it paints — and hit-tests — above
        // the rows it overlaps.
        if let Some(&open_index) = state.menu.path.get(depth) {
            if let Some(&Item::Submenu { items: nested, .. }) = items.get(open_index) {
                panel = panel.child(
                    div()
                        .absolute()
                        .left(metrics.panel_width - metrics.submenu_overlap)
                        // Absolute children sit against the padding box, so
                        // back out the panel's own padding to line the
                        // submenu's first row up with the row that opened it.
                        .top(self.offset_of(items, open_index) - metrics.panel_padding)
                        .child(self.panel::<V>(menu_index, nested, depth + 1, state, cx)),
                );
            }
        }

        panel
    }

    /// Distance from the top of a panel's content box to row `index`. Rows have
    /// fixed heights so submenus can be positioned without measuring.
    fn offset_of(&self, items: &[Item], index: usize) -> Pixels {
        items
            .iter()
            .take(index)
            .map(|item| match item {
                Item::Separator => self.metrics.separator_height,
                _ => self.metrics.row_height,
            })
            .fold(px(0.), |total, height| total + height)
    }

    fn row<V: TitleBarHost>(
        &self,
        menu_index: usize,
        item: &'static Item,
        index: usize,
        depth: usize,
        state: &TitleBarState,
        cx: &mut Context<V>,
    ) -> AnyElement {
        let style = self.style;
        let metrics = self.metrics;
        let id = SharedString::from(format!("titlebar-row-{menu_index}-{depth}-{index}"));

        // Shared layout for every non-separator row.
        let row = || {
            div()
                .h(metrics.row_height)
                .mx_1()
                .px_2()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .rounded_sm()
                .whitespace_nowrap()
        };

        match *item {
            Item::Separator => div()
                .h(metrics.separator_height)
                .px_2()
                .flex()
                .items_center()
                .child(div().h(px(1.)).w_full().bg(rgb(style.border)))
                .into_any_element(),

            Item::Command {
                id: command,
                label,
                shortcut,
                enabled,
            }
            | Item::Toggle {
                id: command,
                label,
                shortcut,
                enabled,
                ..
            } => {
                let check = match *item {
                    Item::Toggle { checked: true, .. } => "✓ ",
                    Item::Toggle { checked: false, .. } => "   ",
                    _ => "",
                };

                row()
                    .id(id)
                    .text_color(rgb(if enabled { style.text } else { style.disabled }))
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(move |s| s.bg(rgb(style.hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    this.title_bar().menu.close();
                                    this.on_menu_command(command, label, window, cx);
                                    cx.notify();
                                }),
                            )
                    })
                    .child(format!("{check}{label}"))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(style.muted))
                            .child(shortcut.unwrap_or("")),
                    )
                    // Moving onto a plain row folds away any deeper submenu.
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        let menu = &mut this.title_bar().menu;
                        if *hovering && menu.path.len() > depth {
                            menu.close_submenus_below(depth);
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            }

            Item::Submenu { label, .. } => {
                let is_open = state.menu.path.get(depth) == Some(&index);

                row()
                    .id(id)
                    .cursor_pointer()
                    .text_color(rgb(style.text))
                    .when(is_open, |row| row.bg(rgb(style.hover)))
                    .hover(move |s| s.bg(rgb(style.hover)))
                    .child(label)
                    .child(div().text_size(px(10.)).child("▸"))
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        let menu = &mut this.title_bar().menu;
                        if *hovering && menu.path.get(depth) != Some(&index) {
                            menu.open_submenu(depth, index);
                            cx.notify();
                        }
                    }))
                    // Clicking works too, for pointer users who don't hover.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.title_bar().menu.open_submenu(depth, index);
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            }
        }
    }

    // --- window controls -------------------------------------------------

    fn window_controls<V: TitleBarHost>(&self, maximized: bool, cx: &mut Context<V>) -> Div {
        let style = self.style;
        let width = self.metrics.control_width;

        let button = move |glyph: &'static str| {
            div()
                .w(width)
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(rgb(style.text))
                .child(glyph)
        };

        div()
            .flex()
            .flex_row()
            .h_full()
            .child(
                button("—")
                    .hover(move |s| s.bg(rgb(style.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            this.title_bar().menu.close();
                            window.minimize_window();
                            cx.notify();
                        }),
                    ),
            )
            .child(
                button(if maximized { "❐" } else { "▢" })
                    .hover(move |s| s.bg(rgb(style.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            this.title_bar().menu.close();
                            toggle_maximize(window);
                            cx.notify();
                        }),
                    ),
            )
            .child(
                button("✕")
                    .hover(move |s| {
                        s.bg(rgb(style.close_hover))
                            .text_color(rgb(style.close_hover_text))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            this.title_bar().menu.close();
                            if this.on_close_requested(window, cx) {
                                window.remove_window();
                            }
                            cx.notify();
                        }),
                    ),
            )
    }

    // --- drag region -----------------------------------------------------

    /// Empty titlebar space: press-and-move drags the window, double-click
    /// toggles maximize. The drag only starts past a small threshold, so the
    /// jitter inside a double-click doesn't turn into a move.
    fn drag_region<V: TitleBarHost>(&self, cx: &mut Context<V>) -> Div {
        let threshold = f32::from(self.metrics.drag_threshold);

        div()
            .h_full()
            .flex()
            .flex_row()
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let state = this.title_bar();
                    let closed = state.menu.close();

                    if event.click_count >= 2 {
                        state.press = None;
                        toggle_maximize(window);
                    } else {
                        state.press = Some(event.position);
                    }

                    if closed {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, window, _| {
                let state = this.title_bar();
                if let Some(start) = state.press {
                    let dx = f32::from(event.position.x - start.x).abs();
                    let dy = f32::from(event.position.y - start.y).abs();
                    if dx + dy > threshold {
                        state.press = None;
                        window_ops::start_drag(window);
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.title_bar().press = None;
                }),
            )
    }
}

/// Maximize / restore, asking the platform what state we're in rather than
/// tracking a flag — that way OS-initiated changes (Win+Up, snapping, the
/// taskbar) stay in sync.
pub fn toggle_maximize(window: &mut Window) {
    if window.is_maximized() {
        window_ops::restore(window);
    } else {
        window_ops::maximize(window);
    }
    window.refresh();
}

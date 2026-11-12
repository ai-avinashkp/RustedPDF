//! Tool strip: what a drag does, the mark colour, and zoom.

use gpui::{
    deferred, div, prelude::*, px, rgb, AnyElement, Context, KeyDownEvent, MouseButton,
    MouseDownEvent, SharedString,
};

use crate::app::Workspace;
use crate::pdf::mark::{picker_colors, OPACITY_STEPS, PRESET_COLORS};
use crate::tab::Tool;
use crate::theme::Palette;
use crate::ui::titlebar::MENU_LAYER;
use crate::view::widgets::{icon_button, section_heading, spacer, swatch};

const TOOLS: &[Tool] = &[Tool::Select, Tool::Highlight, Tool::Underline, Tool::Erase];

impl Workspace {
    pub fn toolbar(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let has_document = !self.tabs.is_empty();
        let color = self.settings.mark_color;

        let mut tools = div().flex().flex_row().items_center().gap(px(2.));
        for tool in TOOLS {
            let tool = *tool;
            tools = tools.child(
                icon_button(
                    format!("tool-{}", tool.label()),
                    tool.glyph(),
                    palette,
                    self.tool == tool,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.set_tool(tool);
                        cx.notify();
                    }),
                ),
            );
        }

        // Quick colours; the full picker lives behind the swatch at the end.
        let mut colors = div().flex().flex_row().items_center().gap(px(4.));
        for (name, preset) in PRESET_COLORS.iter().take(4) {
            let preset = *preset;
            colors = colors.child(
                swatch(format!("swatch-{name}"), preset, preset == color, palette).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.set_mark_color(preset);
                        cx.notify();
                    }),
                ),
            );
        }

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .h(px(34.))
            .px_2()
            .bg(rgb(palette.surface))
            .border_b_1()
            .border_color(rgb(palette.border))
            .text_size(px(12.))
            .child(tools)
            .child(spacer(palette))
            .child(colors)
            .child(self.color_dropdown(palette, cx))
            .child(spacer(palette))
            .child(icon_button("zoom-out", "−", palette, false).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.zoom_step(-1);
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .w(px(48.))
                    .flex()
                    .justify_center()
                    .text_color(rgb(palette.text))
                    .child(format!("{}%", self.zoom_percent())),
            )
            .child(icon_button("zoom-in", "+", palette, false).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.zoom_step(1);
                    cx.notify();
                }),
            ))
            .child(
                icon_button("zoom-reset", "1:1", palette, false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.set_zoom(1.0);
                        cx.notify();
                    }),
                ),
            )
            // Pushes the page navigator to the right.
            .child(div().flex_1())
            .when(has_document, |bar| {
                bar.child(self.page_navigator(palette, cx))
            })
            .into_any_element()
    }

    /// Where you are in the document, and how to get somewhere else: a step
    /// either side of a box you can type a page number into.
    ///
    /// The box doubles as the page counter rather than sitting next to one.
    /// Two controls showing the same number, one of them editable, is the kind
    /// of thing that makes people wonder which is authoritative.
    fn page_navigator(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.tabs[self.active];
        let count = tab.document.page_count();
        let current = tab.current_page() + 1;
        let editing = self.page_input.is_some();

        let mut box_ = div()
            .id("page-box")
            .w(px(46.))
            .h(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(rgb(if editing {
                palette.accent
            } else {
                palette.border
            }))
            .bg(rgb(palette.panel))
            .text_color(rgb(palette.text));

        box_ = match self.page_input.as_deref() {
            Some(typed) => box_
                // Focus is here while the box is being typed into, which is
                // what routes the digits to the handler below instead of to
                // the workspace's shortcuts.
                .track_focus(&self.page_focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if this.edit_page_input(event.keystroke.key.as_str()) {
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                // A caret rather than a real text cursor: the box takes
                // digits and nothing else, so there is no selection or
                // insertion point to represent.
                .child(format!("{typed}|")),
            None => box_
                .cursor_pointer()
                .hover(move |style| style.border_color(rgb(palette.muted)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.open_page_input();
                        cx.notify();
                    }),
                )
                .child(current.to_string()),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .child(
                // The chevrons are small glyphs: at the toolbar's own text size
                // they read as punctuation rather than as something to press.
                icon_button("page-previous", "‹", palette, false)
                    .text_size(px(17.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.step_page(-1);
                            cx.notify();
                        }),
                    ),
            )
            .child(box_)
            .child(
                div()
                    .text_color(rgb(palette.muted))
                    .whitespace_nowrap()
                    .child(format!("of {count}")),
            )
            .child(
                icon_button("page-next", "›", palette, false)
                    .text_size(px(17.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.step_page(1);
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element()
    }

    /// The full colour control: a chip showing the current colour and strength,
    /// opening a dropdown with every colour and a transparency setting.
    ///
    /// A dropdown rather than a dialog — picking a colour is an adjustment you
    /// make while reading, not a decision that should take over the window.
    fn color_dropdown(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let color = self.settings.mark_color;
        let opacity = self.settings.mark_opacity;
        let open = self.color_dropdown_open;

        div()
            .relative()
            .child(
                div()
                    .id("color-chip")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .h(px(26.))
                    .px(px(6.))
                    .rounded_md()
                    .cursor_pointer()
                    .when(open, |chip| chip.bg(rgb(palette.active)))
                    .hover(move |style| style.bg(rgb(palette.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.color_dropdown_open = !this.color_dropdown_open;
                            this.title_bar.menu.close();
                            this.close_page_input();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(18.))
                            .h(px(18.))
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(color)),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(palette.muted))
                            .child(format!("{}%", (opacity * 100.).round())),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(palette.muted))
                            .child("▾"),
                    ),
            )
            .when(open, |wrapper| {
                wrapper.child(
                    deferred(
                        div()
                            .absolute()
                            .top(px(30.))
                            .left(px(0.))
                            .child(self.color_panel(palette, cx)),
                    )
                    .with_priority(MENU_LAYER),
                )
            })
            .into_any_element()
    }

    fn color_panel(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let color = self.settings.mark_color;
        let opacity = self.settings.mark_opacity;

        let mut presets = div().flex().flex_row().flex_wrap().gap(px(6.));
        for (name, preset) in PRESET_COLORS {
            let preset = *preset;
            presets = presets.child(
                swatch(
                    SharedString::from(format!("dropdown-preset-{name}")),
                    preset,
                    preset == color,
                    palette,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.set_mark_color(preset);
                        cx.notify();
                    }),
                ),
            );
        }

        let mut grid = div().flex().flex_row().flex_wrap().gap(px(4.)).w(px(304.));
        for swatch_color in picker_colors() {
            grid = grid.child(
                swatch(
                    SharedString::from(format!("dropdown-color-{swatch_color:06x}")),
                    swatch_color,
                    swatch_color == color,
                    palette,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.set_mark_color(swatch_color);
                        cx.notify();
                    }),
                ),
            );
        }

        // Transparency as steps rather than a slider: each one previews the
        // actual colour at that strength, so you pick what you can see.
        let mut strengths = div().flex().flex_row().items_center().gap(px(4.));
        for step in OPACITY_STEPS {
            let step = *step;
            let selected = (opacity - step).abs() < 0.001;
            strengths = strengths.child(
                div()
                    .id(SharedString::from(format!(
                        "opacity-{}",
                        (step * 100.) as u32
                    )))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .p(px(3.))
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |chip| chip.bg(rgb(palette.hover)))
                    .hover(move |style| style.bg(rgb(palette.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.set_mark_opacity(step);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(38.))
                            .h(px(18.))
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(if selected {
                                palette.text
                            } else {
                                palette.border
                            }))
                            // Over white paper, a multiplied mark at `step`
                            // strength looks like this.
                            .bg(rgb(over_white(color, step))),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(palette.muted))
                            .child(format!("{}%", (step * 100.) as u32)),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(324.))
            .p_3()
            .rounded_md()
            .bg(rgb(palette.panel))
            .border_1()
            .border_color(rgb(palette.border))
            .shadow_lg()
            .text_color(rgb(palette.text))
            // Clicks inside the dropdown must not reach the dismiss backdrop.
            .occlude()
            .child(section_heading("PRESETS", palette))
            .child(presets)
            .child(section_heading("ALL COLOURS", palette))
            .child(grid)
            .child(section_heading("TRANSPARENCY", palette))
            .child(strengths)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(palette.muted))
                    .child(format!(
                        "#{color:06x} at {}% — used for new marks",
                        (opacity * 100.).round()
                    )),
            )
            .into_any_element()
    }
}

/// What `color` multiplied over white paper at `opacity` actually looks like —
/// used to preview a transparency step honestly.
fn over_white(color: u32, opacity: f32) -> u32 {
    let blend = |channel: u32| {
        let tinted = channel as f32;
        (255. + (tinted - 255.) * opacity).round().clamp(0., 255.) as u32
    };

    blend(color >> 16 & 0xff) << 16 | blend(color >> 8 & 0xff) << 8 | blend(color & 0xff)
}

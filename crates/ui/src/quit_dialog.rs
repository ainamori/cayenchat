// SPDX-License-Identifier: GPL-3.0-only
//! The "Quit CayenChat?" question, drawn in the chat window (#378).
//!
//! The native prompt cannot keep both the OS's button order and a safe
//! Enter: the Windows task dialog and AppKit make the first button the
//! default, and that is also the one shown first/rightmost. Here the order
//! comes from [`button_order::row`] and the keyboard focus starts on Cancel,
//! so Enter, Space and Escape cancel on every OS. Quitting needs a click, or
//! Tab to the confirm button and Enter.

use gpui::{prelude::*, *};

use crate::{ChatWindow, button_order, theme};

const QUIT_BOX_MAX_WIDTH: Pixels = px(360.);
/// Space kept free on both sides of the box in a narrow window.
const QUIT_BOX_MARGIN: Pixels = px(32.);
/// Padding and border on both sides of the text inside the box.
const QUIT_BOX_CHROME: Pixels = px(26.);

/// Focus handles of the two buttons, and where the focus was before.
pub(crate) struct QuitDialog {
    cancel: FocusHandle,
    confirm: FocusHandle,
    previous: Option<FocusHandle>,
    focus_pending: bool,
}

impl QuitDialog {
    pub(crate) fn new(cx: &mut Context<ChatWindow>) -> Self {
        Self {
            cancel: cx.focus_handle().tab_stop(true),
            confirm: cx.focus_handle().tab_stop(true),
            previous: None,
            focus_pending: false,
        }
    }
}

impl ChatWindow {
    pub(crate) fn open_quit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quit_prompt_open = true;
        self.quit_dialog.previous = window.focused(cx);
        self.quit_dialog.focus_pending = true;
        cx.notify();
    }

    /// Closes the question; `quit` is whether "Quit" was chosen. Cancelling
    /// puts the focus back where the user was typing.
    fn answer_quit_dialog(&mut self, quit: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.quit_prompt_open {
            return;
        }
        self.quit_prompt_open = false;
        let previous = self.quit_dialog.previous.take();
        if quit {
            cx.quit();
        } else if let Some(handle) = previous {
            window.focus(&handle);
        }
        cx.notify();
    }

    pub(crate) fn render_quit_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.quit_prompt_open {
            return None;
        }
        if self.quit_dialog.focus_pending {
            self.quit_dialog.focus_pending = false;
            window.focus(&self.quit_dialog.cancel);
        }
        let theme = theme::current(cx);
        let connected = self.sessions.values().any(|session| session.irc.is_some());
        let detail = self.i18n.text(if connected {
            "quit_detail_connected"
        } else {
            "quit_detail"
        });
        // A box sized by its content measures wrapped text at the one-line
        // width, so the wrapped lines overflow it. When the detail does not
        // fit on one line it gets the width the capped box offers.
        let detail_width = {
            let style = window.text_style();
            let run = style.to_run(detail.len());
            window
                .text_system()
                .shape_line(
                    detail.clone().into(),
                    style.font_size.to_pixels(window.rem_size()),
                    &[run],
                    None,
                )
                .width
        };
        // The box also stays inside a window narrower than its usual cap.
        let box_width =
            QUIT_BOX_MAX_WIDTH.min((window.viewport_size().width - QUIT_BOX_MARGIN).max(px(0.)));
        let wraps = detail_width > box_width - QUIT_BOX_CHROME;
        let confirm = div()
            .id("quit-dialog-confirm")
            .debug_selector(|| "quit-dialog-confirm".into())
            .track_focus(&self.quit_dialog.confirm)
            .px_2()
            .py_1()
            .bg(theme.selected)
            .cursor_pointer()
            .border_1()
            .border_color(theme.selected)
            .focus(|s| s.border_color(theme.text))
            .child(self.i18n.text("quit_confirm"))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.answer_quit_dialog(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_click(cx.listener(|this, _, window, cx| this.answer_quit_dialog(true, window, cx)));
        let cancel = div()
            .id("quit-dialog-cancel")
            .debug_selector(|| "quit-dialog-cancel".into())
            .track_focus(&self.quit_dialog.cancel)
            .px_2()
            .py_1()
            .border_1()
            .border_color(theme.border)
            .cursor_pointer()
            .focus(|s| s.border_color(theme.text))
            .child(self.i18n.text("cancel"))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.answer_quit_dialog(false, window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_click(
                cx.listener(|this, _, window, cx| this.answer_quit_dialog(false, window, cx)),
            );
        Some(
            div()
                .id("quit-dialog")
                .debug_selector(|| "quit-dialog".into())
                // Nothing behind the question can be clicked or typed into.
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                // Without this the row stretches the box to the window height.
                .items_start()
                .pt(px(16.))
                .bg(Rgba {
                    a: 0.6,
                    ..theme.surface
                })
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => this.answer_quit_dialog(false, window, cx),
                        // Only the two buttons are reachable by Tab.
                        "tab" => {
                            let (cancel, confirm) =
                                (&this.quit_dialog.cancel, &this.quit_dialog.confirm);
                            let next = if cancel.is_focused(window) {
                                confirm
                            } else {
                                cancel
                            };
                            window.focus(next);
                        }
                        _ => {}
                    }
                    cx.stop_propagation();
                }))
                .child(
                    div()
                        .debug_selector(|| "quit-dialog-box".into())
                        // Fit the text; a fixed width left empty space beside it.
                        .max_w(box_width)
                        .h_auto()
                        .p_3()
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
                        .shadow_md()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .child(self.i18n.text("quit_title")),
                        )
                        .child(
                            div()
                                .debug_selector(|| "quit-dialog-detail".into())
                                .when(wraps, |d| d.w(box_width - QUIT_BOX_CHROME))
                                .child(detail),
                        )
                        .child(
                            button_order::row(div().flex().gap_2())
                                .child(confirm)
                                .child(cancel),
                        ),
                )
                .into_any_element(),
        )
    }
}

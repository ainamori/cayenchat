// SPDX-License-Identifier: GPL-3.0-only
//! The toolbar below the menu bar: the everyday actions as icon buttons.
//!
//! A button does what its menu item or shortcut does (it calls the same
//! handler of the chat window), so there is no second implementation. Whether
//! a button can be pressed is worked out in [`State`] when the chat state
//! changes; drawing only reads it. The bar is a cached view of its own, so
//! typing and new messages do not lay it out again.

use std::borrow::Cow;

use gpui::{prelude::*, *};

use crate::{ChatWindow, Connect, Disconnect, OpenSettings, ircv3_settings::TextTooltip};
use crate::{key_recorder, shortcuts, theme::Theme};
use cayenchat_app::Command;

/// Height of the bar: the 28 px buttons with a little room around them.
pub const HEIGHT: Pixels = px(34.);
const BUTTON: f32 = 28.;
const ICON: f32 = 16.;

/// What the buttons can do right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The first button reconnects (the selected server has been connected in
    /// this run) instead of connecting; it has another icon and name.
    pub reconnect: bool,
    pub can_connect: bool,
    pub can_disconnect: bool,
    /// The selected channel is joined: the button parts instead of joining.
    pub joined: bool,
    pub can_join_or_part: bool,
    pub can_next_unread: bool,
    pub can_whois: bool,
    pub can_upload: bool,
}

#[derive(Clone, Copy)]
enum Click {
    Connect,
    Disconnect,
    JoinOrPart,
    NextUnread,
    Whois,
    Upload,
    Settings,
}

/// Where the shortcut shown in a tooltip comes from.
#[derive(Clone, Copy)]
enum Keys {
    None,
    /// A navigation action, whose keys the user can change.
    Action(&'static str),
    /// A shortcut that cannot be changed.
    Fixed(&'static str),
}

struct Button {
    id: &'static str,
    icon: &'static str,
    /// Locale key of the action's name.
    label: &'static str,
    keys: Keys,
    enabled: bool,
    click: Click,
}

impl State {
    /// The buttons in order, with `None` for a separator between groups.
    /// Topic, mode and nickname changes and the next notification join the
    /// groups when those actions exist (#369, #370).
    fn layout(&self) -> Vec<Option<Button>> {
        let button = |id, icon, label, keys, enabled, click| {
            Some(Button {
                id,
                icon,
                label,
                keys,
                enabled,
                click,
            })
        };
        vec![
            if self.reconnect {
                button(
                    "connect",
                    "refresh-cw",
                    "reconnect",
                    Keys::None,
                    self.can_connect,
                    Click::Connect,
                )
            } else {
                button(
                    "connect",
                    "plug-zap",
                    "connect",
                    Keys::None,
                    self.can_connect,
                    Click::Connect,
                )
            },
            button(
                "disconnect",
                "unplug",
                "disconnect",
                Keys::None,
                self.can_disconnect,
                Click::Disconnect,
            ),
            None,
            if self.joined {
                button(
                    "join-part",
                    "log-out",
                    "toolbar_part",
                    Keys::None,
                    self.can_join_or_part,
                    Click::JoinOrPart,
                )
            } else {
                button(
                    "join-part",
                    "log-in",
                    "toolbar_join",
                    Keys::None,
                    self.can_join_or_part,
                    Click::JoinOrPart,
                )
            },
            None,
            button(
                "next-unread",
                "chevrons-down",
                "shortcut_next_unread_channel",
                Keys::Action("next_unread_channel"),
                self.can_next_unread,
                Click::NextUnread,
            ),
            None,
            button(
                "whois",
                "user-search",
                "toolbar_whois",
                Keys::None,
                self.can_whois,
                Click::Whois,
            ),
            button(
                "upload",
                "image-up",
                "toolbar_upload",
                Keys::None,
                self.can_upload,
                Click::Upload,
            ),
            None,
            button(
                "settings",
                "settings",
                "menu_settings",
                Keys::Fixed(if cfg!(target_os = "macos") {
                    "cmd-,"
                } else {
                    "ctrl-,"
                }),
                true,
                Click::Settings,
            ),
        ]
    }
}

/// The tooltip: the action's name and the keys that run it.
fn tooltip_text(name: &str, keys: &[String]) -> String {
    if keys.is_empty() {
        name.to_owned()
    } else {
        format!("{name} ({})", keys.join(", "))
    }
}

/// The keys of `keys` now, labelled for display.
fn shown_keys(keys: Keys, overrides: &shortcuts::Overrides) -> Vec<String> {
    let keys: Vec<String> = match keys {
        Keys::None => Vec::new(),
        Keys::Fixed(key) => vec![key.to_owned()],
        Keys::Action(id) => shortcuts::ACTIONS
            .iter()
            .find(|action| action.id == id)
            .map(|action| shortcuts::keys(action, overrides))
            .unwrap_or_default(),
    };
    keys.iter().map(|key| key_recorder::describe(key)).collect()
}

struct Toolbar {
    chat: WeakEntity<ChatWindow>,
    state: State,
    _subscriptions: [Subscription; 2],
}

/// The toolbar's view, showing `state` until the chat window says otherwise.
pub fn view(state: State, cx: &mut Context<ChatWindow>) -> AnyView {
    let chat = cx.entity();
    cx.new(|cx: &mut Context<Toolbar>| Toolbar {
        chat: chat.downgrade(),
        state,
        _subscriptions: [
            // The state is worked out when the chat window changes, and the
            // bar redraws only if a button's availability or name did.
            cx.observe(&chat, |this, chat, cx| {
                let state = chat.read(cx).toolbar_state();
                if state != this.state {
                    this.state = state;
                    cx.notify();
                }
            }),
            // Light or dark, or other colors.
            cx.observe_global::<Theme>(|_, cx| cx.notify()),
        ],
    })
    .into()
}

impl Toolbar {
    fn button(&self, spec: Button, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let Button {
            id,
            icon,
            label,
            keys,
            enabled,
            click,
        } = spec;
        let chat = self.chat.clone();
        let color = if enabled {
            theme.text
        } else {
            theme.text_muted
        };
        div()
            .id(id)
            .debug_selector(|| format!("toolbar-{id}"))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(px(BUTTON))
            .rounded(px(4.))
            .child(
                svg()
                    .path(format!("icons/ui/{icon}.svg"))
                    .size(px(ICON))
                    .text_color(color),
            )
            .when(enabled, |button| {
                let hover = theme.hover_strong;
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let _ = this.chat.update(cx, |chat, cx| match click {
                            Click::Connect => chat.connect_action(&Connect, window, cx),
                            Click::Disconnect => chat.disconnect_action(&Disconnect, window, cx),
                            Click::JoinOrPart => chat.toolbar_join_or_part(cx),
                            Click::NextUnread => {
                                chat.dispatch(Command::NextUnreadChannel, window, cx)
                            }
                            Click::Whois => chat.toolbar_whois(cx),
                            Click::Upload => chat.toolbar_upload(window, cx),
                            Click::Settings => chat.open_settings(&OpenSettings, window, cx),
                        });
                    }))
            })
            // Built when the pointer rests on the button, so a changed
            // shortcut or language shows without redrawing the bar.
            .tooltip(move |_, cx| {
                let name = chat
                    .read_with(cx, |chat, _| chat.i18n.text(label))
                    .unwrap_or_default();
                let overrides = cx
                    .try_global::<crate::ShortcutPrefs>()
                    .map(|prefs| prefs.overrides.clone())
                    .unwrap_or_default();
                let text = tooltip_text(&name, &shown_keys(keys, &overrides));
                cx.new(|_| TextTooltip(text.into())).into()
            })
    }
}

impl Render for Toolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::current(cx);
        let mut bar = div()
            .id("toolbar")
            .flex()
            .items_center()
            .gap(px(2.))
            .w_full()
            .h(HEIGHT)
            .px(px(6.))
            .flex_shrink_0()
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border);
        for spec in self.state.layout() {
            bar = match spec {
                Some(spec) => bar.child(self.button(spec, &theme, cx)),
                None => bar.child(
                    div()
                        .w(px(1.))
                        .h(px(ICON))
                        .mx(px(4.))
                        .flex_shrink_0()
                        .bg(theme.separator),
                ),
            };
        }
        bar
    }
}

/// The icons, compiled in: no files are read at run time.
pub struct Icons;

const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/ui/chevrons-down.svg",
        include_bytes!("../../../assets/icons/ui/chevrons-down.svg"),
    ),
    (
        "icons/ui/image-up.svg",
        include_bytes!("../../../assets/icons/ui/image-up.svg"),
    ),
    (
        "icons/ui/log-in.svg",
        include_bytes!("../../../assets/icons/ui/log-in.svg"),
    ),
    (
        "icons/ui/log-out.svg",
        include_bytes!("../../../assets/icons/ui/log-out.svg"),
    ),
    (
        "icons/ui/plug-zap.svg",
        include_bytes!("../../../assets/icons/ui/plug-zap.svg"),
    ),
    (
        "icons/ui/refresh-cw.svg",
        include_bytes!("../../../assets/icons/ui/refresh-cw.svg"),
    ),
    (
        "icons/ui/settings.svg",
        include_bytes!("../../../assets/icons/ui/settings.svg"),
    ),
    (
        "icons/ui/unplug.svg",
        include_bytes!("../../../assets/icons/ui/unplug.svg"),
    ),
    (
        "icons/ui/user-search.svg",
        include_bytes!("../../../assets/icons/ui/user-search.svg"),
    ),
];

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, Icons, Keys, State, key_recorder, shortcuts, shown_keys, tooltip_text};
    use gpui::AssetSource;

    fn buttons(state: State) -> Vec<Button> {
        state.layout().into_iter().flatten().collect()
    }

    #[test]
    fn tooltips_name_the_action_and_its_keys() {
        assert_eq!(tooltip_text("Disconnect", &[]), "Disconnect");
        assert_eq!(
            tooltip_text(
                "Next unread channel",
                &["Ctrl+Tab".into(), "Alt+Space".into()]
            ),
            "Next unread channel (Ctrl+Tab, Alt+Space)"
        );
    }

    #[test]
    fn a_changed_navigation_shortcut_shows_in_the_tooltip() {
        let mut overrides = shortcuts::Overrides::new();
        overrides.insert("next_unread_channel".into(), "ctrl-j".into());
        assert_eq!(
            shown_keys(Keys::Action("next_unread_channel"), &overrides),
            vec![key_recorder::describe("ctrl-j")]
        );
        assert_ne!(
            shown_keys(
                Keys::Action("next_unread_channel"),
                &shortcuts::Overrides::new()
            ),
            shown_keys(Keys::Action("next_unread_channel"), &overrides)
        );
        assert!(shown_keys(Keys::None, &overrides).is_empty());
    }

    #[test]
    fn the_first_button_follows_whether_the_server_was_connected() {
        let first = |state| buttons(state).remove(0);
        assert_eq!(first(State::default()).icon, "plug-zap");
        let reconnect = first(State {
            reconnect: true,
            ..State::default()
        });
        assert_eq!(
            (reconnect.icon, reconnect.label),
            ("refresh-cw", "reconnect")
        );
    }

    #[test]
    fn the_channel_button_joins_or_parts() {
        let channel = |joined| {
            buttons(State {
                joined,
                ..State::default()
            })
            .into_iter()
            .find(|button| button.id == "join-part")
            .map(|button| button.icon)
        };
        assert_eq!(channel(false), Some("log-in"));
        assert_eq!(channel(true), Some("log-out"));
    }

    #[test]
    fn unavailable_buttons_are_disabled_and_settings_never_is() {
        let state = State::default();
        for button in buttons(state) {
            assert_eq!(button.enabled, button.id == "settings", "{}", button.id);
        }
    }

    #[test]
    fn every_icon_used_is_compiled_in() {
        let icons = Icons;
        for state in [
            State::default(),
            State {
                reconnect: true,
                joined: true,
                ..State::default()
            },
        ] {
            for button in buttons(state) {
                let loaded = icons
                    .load(&format!("icons/ui/{}.svg", button.icon))
                    .unwrap();
                assert!(loaded.is_some(), "{}", button.icon);
            }
        }
    }
}

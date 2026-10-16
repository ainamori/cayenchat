// SPDX-License-Identifier: GPL-3.0-only
//! Channel keys (`+k`): asking for one when a JOIN is refused with 475, and
//! keeping it in the credential store once the JOIN worked.
//!
//! A JOIN we sent is tracked in [`ServerSession::key_joins`] until the
//! server answers it. Only a 475 for a tracked JOIN opens the key dialog, so
//! another server's or another channel's refusal never does. The key typed
//! in the dialog is saved only when our own JOIN is reported back, never
//! when it was merely sent or refused. A key never reaches the settings
//! file, a log, the diagnostics or the input history.

use std::collections::BTreeMap;

use cayenchat_irc_core::{ConnectionConfig, valid_channel_key};
use cayenchat_model::NetworkId;
use cayenchat_storage::{CredentialStore, Secret, SecretKey, ServerProfile};
use gpui::{prelude::*, *};

use crate::{ChatWindow, button_order, input::TextInput, secrets, settings_theme, theme};

/// JOINs tracked per server, so a long auto-join list does not grow it
/// without bound.
const MAX_TRACKED: usize = 256;
/// Channels one `/join` may name for a stored key to be looked up for each.
const MAX_JOIN_CHANNELS: usize = 32;

/// A JOIN waiting for the server's answer.
pub struct KeyJoin {
    /// The key typed in the dialog for this attempt, if it came from there.
    pub typed: Option<Secret>,
    /// Whether to save `typed` once the JOIN worked.
    pub remember: bool,
    /// A saved key was used (now or in an earlier attempt of this dialog)
    /// and the server refused it; it is replaced or removed by the outcome.
    pub stale: bool,
}

/// The dialog asking for the key of one channel on one server.
pub struct KeyPrompt {
    pub network: NetworkId,
    pub channel: String,
    folded: String,
    stale: bool,
    rejected: bool,
    remember: bool,
    input: Entity<TextInput>,
    error: Option<String>,
    focus_pending: bool,
}

/// The channels of a plain `/join #a,#b` (no key given), or `None` for any
/// other form of the command, which is sent as typed.
pub fn plain_join_channels(text: &str) -> Option<Vec<String>> {
    let mut words = text.strip_prefix('/')?.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("join") {
        return None;
    }
    let channels = words.next()?;
    if words.next().is_some() {
        return None;
    }
    let channels: Vec<String> = channels.split(',').map(str::to_owned).collect();
    (channels.len() <= MAX_JOIN_CHANNELS
        && channels
            .iter()
            .all(|name| cayenchat_irc_core::valid_channel(name)))
    .then_some(channels)
}

/// Gives `config` the saved keys of the auto-join channels of `profile`. A
/// store that cannot be read, or a saved key the connection cannot send,
/// just means no key: the 475 then asks for it.
pub fn use_stored_keys(
    config: &mut ConnectionConfig,
    profile: &ServerProfile,
    store: &CredentialStore,
) {
    config.channel_keys = stored_channel_keys(profile, store);
    if config.validate().is_err() {
        config.channel_keys.clear();
    }
}

fn stored_channel_keys(
    profile: &ServerProfile,
    store: &CredentialStore,
) -> BTreeMap<String, String> {
    profile
        .channels()
        .into_iter()
        .filter_map(|channel| {
            let key = store
                .get(&SecretKey::channel_key(&profile.id, &channel))
                .ok()??;
            valid_channel_key(key.expose()).then(|| (channel, key.expose().to_owned()))
        })
        .collect()
}

impl ChatWindow {
    /// Sends `JOIN` for each channel with its saved key, if there is one.
    pub(crate) fn join_channels(
        &mut self,
        network: NetworkId,
        channels: &[String],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let profile = self
            .sessions
            .get(&network)
            .map(|session| session.profile_id.clone())
            .ok_or_else(|| self.i18n.text("not_connected"))?;
        let store = secrets::store(cx);
        for channel in channels {
            let saved = store
                .get(&SecretKey::channel_key(&profile, channel))
                .ok()
                .flatten()
                .filter(|key| valid_channel_key(key.expose()));
            self.registered_connection(network)?
                .join_channel(channel, saved.as_ref().map(Secret::expose))?;
            self.note_join(network, channel, saved.is_some());
        }
        Ok(())
    }

    /// Remembers a JOIN just sent: a refusal of a channel with no conversation
    /// yet can be shown, and a 475 for it asks for the key. `stale` says a
    /// saved key was sent.
    pub(crate) fn note_join(&mut self, network: NetworkId, channel: &str, stale: bool) {
        let folded = self.state.casemapping(network).fold(channel);
        let Some(session) = self.sessions.get_mut(&network) else {
            return;
        };
        if session.pending_joins.len() >= 32 {
            session.pending_joins.clear();
        }
        session.pending_joins.insert(folded.clone());
        track(session, folded, KeyJoin::untyped(stale));
    }

    /// A new connection registered: its auto-join JOINs (with the saved keys
    /// it was configured with) are about to be answered.
    pub(crate) fn start_key_tracking(&mut self, network: NetworkId) {
        self.key_prompts.retain(|prompt| prompt.network != network);
        let casemapping = self.state.casemapping(network);
        let Some(session) = self.sessions.get_mut(&network) else {
            return;
        };
        session.key_joins.clear();
        let Some(config) = session.active_config.as_ref() else {
            return;
        };
        let joins: Vec<(String, bool)> = config
            .channels
            .iter()
            .map(|channel| {
                (
                    casemapping.fold(channel),
                    config.channel_keys.contains_key(channel),
                )
            })
            .collect();
        for (folded, stale) in joins {
            track(session, folded, KeyJoin::untyped(stale));
        }
    }

    /// The connection ended: its JOINs will never be answered.
    pub(crate) fn end_key_tracking(&mut self, network: NetworkId) {
        self.key_prompts.retain(|prompt| prompt.network != network);
        if let Some(session) = self.sessions.get_mut(&network) {
            session.key_joins.clear();
        }
    }

    /// The server refused our JOIN of `channel`. Only 475 for a JOIN we
    /// sent asks for a key; 471, 473, 474 and the like cannot be cured by one.
    pub(crate) fn key_join_refused(
        &mut self,
        network: NetworkId,
        channel: &str,
        bad_key: bool,
        cx: &mut Context<Self>,
    ) {
        let folded = self.state.casemapping(network).fold(channel);
        let Some(join) = self
            .sessions
            .get_mut(&network)
            .and_then(|session| session.key_joins.remove(&folded))
        else {
            return;
        };
        if !bad_key {
            return;
        }
        let rejected = join.typed.is_some();
        if let Some(prompt) = self
            .key_prompts
            .iter_mut()
            .find(|prompt| prompt.network == network && prompt.folded == folded)
        {
            prompt.stale |= join.stale;
            prompt.rejected = rejected;
            prompt.focus_pending = true;
            return;
        }
        let placeholder = self.i18n.text("key_prompt_placeholder");
        let input = cx.new(|cx| TextInput::new_field(&placeholder, "", true, cx));
        self.key_prompts.push(KeyPrompt {
            network,
            channel: channel.to_owned(),
            folded,
            stale: join.stale,
            rejected,
            remember: true,
            input,
            error: None,
            focus_pending: true,
        });
        self.server_menu = None;
        self.member_menu = None;
        self.channel_menu = None;
        self.log_menu = None;
        cx.notify();
    }

    /// We are in `channel`: the JOIN worked, so the key typed for it may now
    /// be saved, or a saved key that stopped working removed.
    pub(crate) fn key_join_succeeded(
        &mut self,
        network: NetworkId,
        channel: &str,
        cx: &mut Context<Self>,
    ) {
        let folded = self.state.casemapping(network).fold(channel);
        let Some(session) = self.sessions.get_mut(&network) else {
            return;
        };
        let profile = session.profile_id.clone();
        let Some(join) = session.key_joins.remove(&folded) else {
            return;
        };
        let store = secrets::store(cx);
        let key = SecretKey::channel_key(&profile, channel);
        let failure = match (&join.typed, join.remember) {
            (Some(typed), true) => store
                .set(&key, typed)
                .err()
                .map(|error| ("channel_key_save_failed", error)),
            _ if join.stale && join.typed.is_some() => store
                .delete(&key)
                .err()
                .map(|error| ("channel_key_forget_failed", error)),
            _ => None,
        };
        let saved = match (&join.typed, join.remember, &failure) {
            (Some(typed), true, None) => Some(Some(typed.expose().to_owned())),
            (Some(_), false, None) if join.stale => Some(None),
            _ => None,
        };
        if let Some(saved) = saved {
            self.sync_config_key(network, channel, saved);
        }
        if let Some((message, error)) = failure {
            let text = self.i18n.format(
                message,
                &[
                    (
                        "channel",
                        &cayenchat_model::display::neutralize_bidi(channel),
                    ),
                    ("error", &secrets::error_text(&self.i18n, &error)),
                ],
            );
            self.state.append_server_message(network, text.clone());
            self.set_feedback(Some(text));
        }
    }

    /// Keeps the configuration reconnects reuse in step with the store.
    fn sync_config_key(&mut self, network: NetworkId, channel: &str, key: Option<String>) {
        let Some(config) = self
            .sessions
            .get_mut(&network)
            .and_then(|session| session.active_config.as_mut())
        else {
            return;
        };
        let Some(name) = config
            .channels
            .iter()
            .find(|name| name.eq_ignore_ascii_case(channel))
            .cloned()
        else {
            return;
        };
        match key {
            Some(key) => config.channel_keys.insert(name, key),
            None => config.channel_keys.remove(&name),
        };
    }

    /// The server whose key field has focus, with the channel.
    pub(crate) fn focused_key_prompt(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<(NetworkId, String)> {
        self.key_prompts
            .iter()
            .find(|prompt| prompt.input.read(cx).focus_handle(cx).is_focused(window))
            .map(|prompt| (prompt.network, prompt.folded.clone()))
    }

    pub(crate) fn submit_key_prompt(
        &mut self,
        network: NetworkId,
        folded: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .key_prompts
            .iter()
            .position(|prompt| prompt.network == network && prompt.folded == folded)
        else {
            return;
        };
        let prompt = &self.key_prompts[index];
        if prompt.input.read(cx).is_composing() {
            return;
        }
        let key = Secret::new(prompt.input.read(cx).text().trim());
        let channel = prompt.channel.clone();
        let (remember, stale) = (prompt.remember, prompt.stale);
        let sent = if key.is_empty() {
            Err(self.i18n.text("key_prompt_required"))
        } else if !valid_channel_key(key.expose()) {
            Err(self.i18n.text("key_prompt_invalid"))
        } else {
            self.registered_connection(network)
                .and_then(|connection| connection.join_channel(&channel, Some(key.expose())))
        };
        match sent {
            Ok(()) => {
                let session = self.sessions.get_mut(&network);
                if let Some(session) = session {
                    if session.pending_joins.len() >= 32 {
                        session.pending_joins.clear();
                    }
                    session.pending_joins.insert(folded.to_owned());
                    track(
                        session,
                        folded.to_owned(),
                        KeyJoin {
                            typed: Some(key),
                            remember,
                            stale,
                        },
                    );
                }
                self.close_key_prompt(index, window, cx);
            }
            Err(error) => self.key_prompts[index].error = Some(error),
        }
        cx.notify();
    }

    /// Gives up: nothing is joined or saved.
    pub(crate) fn cancel_key_prompt(
        &mut self,
        network: NetworkId,
        folded: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .key_prompts
            .iter()
            .position(|prompt| prompt.network == network && prompt.folded == folded)
        {
            self.close_key_prompt(index, window, cx);
        }
        cx.notify();
    }

    fn close_key_prompt(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.key_prompts.remove(index);
        match self.key_prompts.first() {
            Some(next) => window.focus(&next.input.focus_handle(cx)),
            None => window.focus(&self.inputs[&self.state.selection()].focus_handle(cx)),
        }
    }

    /// One centered dialog with a row per channel waiting for a key.
    pub(crate) fn render_key_prompts(
        &mut self,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.key_prompts.is_empty() {
            return None;
        }
        let typing = self.focused_key_prompt(window, cx).is_some();
        let pending = self
            .key_prompts
            .iter()
            .position(|prompt| prompt.focus_pending);
        for prompt in &mut self.key_prompts {
            prompt.focus_pending = false;
        }
        if let Some(index) = pending
            && !typing
        {
            window.focus(&self.key_prompts[index].input.focus_handle(cx));
        }
        let theme = theme::current(cx);
        let border = theme.border;
        let viewport = window.viewport_size();
        let width = px(360.);
        let mut dialog = div()
            .id("key-prompts")
            .absolute()
            .left(((viewport.width - width) / 2.).max(px(0.)) - origin.x)
            .top(px(60.).min(viewport.height / 4.) - origin.y)
            .w(width)
            .p_2()
            .bg(theme.surface)
            .border_1()
            .border_color(border)
            .shadow_md()
            .flex()
            .flex_col()
            .gap_2();
        for (index, prompt) in self.key_prompts.iter().enumerate() {
            let network = prompt.network;
            let folded = prompt.folded.clone();
            let server = self
                .state
                .networks()
                .iter()
                .find(|server| server.id == network)
                .map(|server| server.name.clone())
                .unwrap_or_default();
            let title = self.i18n.format(
                "key_prompt_title",
                &[(
                    "channel",
                    &cayenchat_model::display::neutralize_bidi(&prompt.channel),
                )],
            );
            let hint = if prompt.rejected {
                Some(self.i18n.text("key_prompt_rejected"))
            } else if prompt.stale {
                Some(self.i18n.text("key_prompt_stale"))
            } else {
                None
            };
            let remember = prompt.remember;
            let (submit_folded, cancel_folded, toggle_folded) =
                (folded.clone(), folded.clone(), folded.clone());
            dialog = dialog
                .when(index > 0, |d| {
                    d.child(div().border_t_1().border_color(theme.separator))
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_weight(FontWeight::BOLD).child(server))
                        .child(title)
                        .when_some(hint, |d, hint| {
                            d.child(div().text_color(theme.warning).child(hint))
                        })
                        .child(prompt.input.clone())
                        .child(
                            div()
                                .id(("key-prompt-remember", index))
                                .flex()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .child(settings_theme::checkbox(remember, true, cx))
                                .child(self.i18n.text("key_prompt_remember"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(prompt) = this
                                        .key_prompts
                                        .iter_mut()
                                        .find(|p| p.network == network && p.folded == toggle_folded)
                                    {
                                        prompt.remember = !prompt.remember;
                                    }
                                    cx.notify();
                                })),
                        )
                        .when_some(prompt.error.clone(), |d, error| {
                            d.child(div().text_color(theme.warning).child(error))
                        })
                        .child(
                            button_order::row(div().flex().gap_2())
                                .child(
                                    div()
                                        .id(("key-prompt-submit", index))
                                        .px_2()
                                        .py_1()
                                        .bg(theme.selected)
                                        .cursor_pointer()
                                        .child(self.i18n.text("key_prompt_submit"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.submit_key_prompt(
                                                network,
                                                &submit_folded,
                                                window,
                                                cx,
                                            )
                                        })),
                                )
                                .child(
                                    div()
                                        .id(("key-prompt-cancel", index))
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(border)
                                        .cursor_pointer()
                                        .child(self.i18n.text("key_prompt_cancel"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.cancel_key_prompt(
                                                network,
                                                &cancel_folded,
                                                window,
                                                cx,
                                            )
                                        })),
                                ),
                        ),
                );
        }
        Some(dialog.into_any_element())
    }
}

impl KeyPrompt {
    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }
}

impl KeyJoin {
    fn untyped(stale: bool) -> Self {
        Self {
            typed: None,
            remember: false,
            stale,
        }
    }
}

fn track(session: &mut crate::session::ServerSession, folded: String, join: KeyJoin) {
    if session.key_joins.len() < MAX_TRACKED || session.key_joins.contains_key(&folded) {
        session.key_joins.insert(folded, join);
    }
}

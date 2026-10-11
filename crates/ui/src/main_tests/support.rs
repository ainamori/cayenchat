// SPDX-License-Identifier: GPL-3.0-only
use crate::Settings;

/// Default settings whose selected server auto-joins `channels`.
pub(crate) fn settings_with_channels(channels: &str) -> Settings {
    let mut settings = Settings::default();
    settings
        .add_server(cayenchat_storage::PRESETS[0].host)
        .channels = channels.into();
    settings
}

// SPDX-License-Identifier: GPL-3.0-only
use super::{OpenSettings, Quit, app_menus};
use crate::localization::Localizer;
use cayenchat_storage::Language;
use gpui::{Action, MenuItem};

fn actions(items: &[MenuItem]) -> Vec<&'static str> {
    items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { action, .. } => Some(action.name()),
            _ => None,
        })
        .collect()
}

#[test]
fn settings_is_offered_once_and_the_window_menu_is_macos_only() {
    for language in [Language::English, Language::Japanese] {
        let menus = app_menus(false, &Localizer::new(language));
        let mut all: Vec<&str> = Vec::new();
        for menu in &menus {
            all.extend(actions(&menu.items));
        }
        let count = |name: &str| all.iter().filter(|action| **action == name).count();
        assert_eq!(count(OpenSettings.name()), 1);
        assert_eq!(count(Quit.name()), 1);
        // The first menu holds Settings and Quit on every platform.
        let first = actions(&menus[0].items);
        assert_eq!(first.first(), Some(&OpenSettings.name()));
        assert_eq!(first.last(), Some(&Quit.name()));
        assert_eq!(
            menus.iter().any(|menu| menu.menu_type.is_some()),
            cfg!(target_os = "macos")
        );
    }
}

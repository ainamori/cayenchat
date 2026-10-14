// SPDX-License-Identifier: GPL-3.0-only
//! Left-to-right order of a confirm button and Cancel (#378).
//!
//! Windows and Linux put the confirming action first (OK, Cancel); macOS puts
//! Cancel first (Cancel, OK). Rows drawn by the app list the confirm button
//! first in the source and call [`row`]; on macOS the row is reversed, so
//! the source order, tab order and handlers stay the same on every OS.
//!
//! Native prompts (`Window::prompt`) always pass `[confirm, Cancel]` and need
//! no help: Windows' task dialog shows custom buttons left to right, AppKit
//! lays the first button out at the right edge, and the Linux fallback
//! prompt stacks them top to bottom. Answer 0 is therefore always the
//! confirming action and the last answer is Cancel.

use gpui::{Div, Styled};

/// Whether this OS shows Cancel to the left of the confirm button.
pub(crate) const fn cancel_first() -> bool {
    cancel_first_on(cfg!(target_os = "macos"))
}

const fn cancel_first_on(macos: bool) -> bool {
    macos
}

/// Lays a row out with its first child (the confirm button) in the OS's place.
pub(crate) fn row(row: Div) -> Div {
    row_with(row, cancel_first())
}

/// [`row`] for a given OS, so that both orders can be drawn in tests.
pub(crate) fn row_with(row: Div, cancel_first: bool) -> Div {
    if cancel_first {
        row.flex_row_reverse()
    } else {
        row
    }
}

#[cfg(test)]
mod tests {
    use super::cancel_first_on;

    #[test]
    fn only_macos_puts_cancel_first() {
        assert!(cancel_first_on(true));
        assert!(!cancel_first_on(false));
    }
}

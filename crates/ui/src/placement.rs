// SPDX-License-Identifier: GPL-3.0-only
//! Putting a new window where it was laid out on Windows, whose scale can
//! differ between displays. The arithmetic stays in [`crate::window_layout`].

#[cfg(any(target_os = "windows", test))]
use gpui::{Bounds, Pixels};

/// A logical rectangle as the physical pixels of a display with `scale`.
#[cfg(any(target_os = "windows", test))]
pub fn physical_rect(bounds: Bounds<Pixels>, scale: f32) -> [i32; 4] {
    let physical = |value: Pixels| (f32::from(value) * scale).round() as i32;
    [
        physical(bounds.origin.x),
        physical(bounds.origin.y),
        physical(bounds.size.width),
        physical(bounds.size.height),
    ]
}

/// A new window is created on whichever display Windows picks by default, and
/// GPUI turns the requested logical bounds into pixels with the scale of that
/// display, not of the target one. With different scales the window would land
/// elsewhere, so it is put where wanted in physical pixels, as laid out in the
/// owner's `scale`, once it exists. Best effort: failures leave it as created.
#[cfg(target_os = "windows")]
pub fn place_native(window: &gpui::Window, bounds: Bounds<Pixels>, scale: f32) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SetWindowPos,
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(native) = handle.as_raw() else {
        return;
    };
    let hwnd = native.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let [x, y, width, height] = physical_rect(bounds, scale);
    // SAFETY: plain queries and a move of a live window with local structs.
    unsafe {
        let (mut outer, mut inner): (RECT, RECT) = std::mem::zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        if GetWindowRect(hwnd, &mut outer) == 0
            || GetClientRect(hwnd, &mut inner) == 0
            || ClientToScreen(hwnd, &mut origin) == 0
        {
            return;
        }
        // The wanted rectangle is the content's; the window adds its frame.
        let (left, top) = (origin.x - outer.left, origin.y - outer.top);
        let extra_width = (outer.right - outer.left) - (inner.right - inner.left);
        let extra_height = (outer.bottom - outer.top) - (inner.bottom - inner.top);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x - left,
            y - top,
            width + extra_width,
            height + extra_height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn physical_rect_uses_the_given_scale() {
        // Laid out at 200%: a logical 1230 is physical 2460, whatever the
        // scale of the display the window happens to be created on.
        let bounds = Bounds::new(point(px(1230.), px(20.)), size(px(900.), px(750.)));
        assert_eq!(physical_rect(bounds, 2.0), [2460, 40, 1800, 1500]);
        assert_eq!(physical_rect(bounds, 1.0), [1230, 20, 900, 750]);
    }

    #[test]
    fn physical_rect_keeps_negative_origins() {
        let bounds = Bounds::new(point(px(-1000.), px(-10.)), size(px(500.), px(400.)));
        assert_eq!(physical_rect(bounds, 1.5), [-1500, -15, 750, 600]);
    }
}

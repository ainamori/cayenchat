// SPDX-License-Identifier: GPL-3.0-only
//! What GPUI does not tell about where a new window may go, read from the
//! platform: the owner's real position, the work area (the monitor less
//! taskbar, Dock and panels) of the monitor it is on, and on X11 moving a
//! window once the window manager has taken it over. The arithmetic stays in
//! [`crate::window_layout`]; this is the narrow native boundary, and every
//! failure answers `None` so the caller falls back to what GPUI reports.

use gpui::{Bounds, Pixels, Window};

/// Where the owner window is and the area around it a new window may use,
/// both in GPUI's logical pixels in the coordinates of `Window::bounds`.
pub struct Anchor {
    pub owner: Bounds<Pixels>,
    /// The work area of the owner's monitor; `None` when the platform cannot
    /// say (then the caller uses the whole display, panels not excluded).
    pub work: Option<Bounds<Pixels>>,
}

#[cfg(target_os = "windows")]
pub fn anchor(window: &Window, _title: &str) -> Anchor {
    use gpui::{point, px, size};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };

    let owner = window.bounds();
    let work = (|| {
        if cfg!(test) {
            return None;
        }
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(native) = handle.as_raw() else {
            return None;
        };
        let hwnd = native.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        // SAFETY: plain queries on a live window handle and a zeroed out struct.
        let work = unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return None;
            }
            info.rcWork
        };
        // Physical pixels to GPUI's logical ones, as for its display bounds.
        let scale = window.scale_factor();
        Some(Bounds::new(
            point(px(work.left as f32 / scale), px(work.top as f32 / scale)),
            size(
                px((work.right - work.left) as f32 / scale),
                px((work.bottom - work.top) as f32 / scale),
            ),
        ))
    })();
    Anchor { owner, work }
}

/// GPUI's mac bounds are relative to the window's own screen with the origin
/// at its top left, so the visible frame (without menu bar and Dock) is
/// converted to that.
#[cfg(target_os = "macos")]
pub fn anchor(window: &Window, _title: &str) -> Anchor {
    use gpui::{point, px, size};
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let owner = window.bounds();
    let work =
        (|| {
            let handle = HasWindowHandle::window_handle(window).ok()?;
            let RawWindowHandle::AppKit(native) = handle.as_raw() else {
                return None;
            };
            // SAFETY: the view of the live window the handle was just taken from;
            // this runs on the main thread, where GPUI's windows are used.
            let view: &NSView = unsafe { native.ns_view.cast::<NSView>().as_ref() };
            let screen = view.window()?.screen()?;
            let (frame, visible) = (screen.frame(), screen.visibleFrame());
            Some(Bounds::new(
                point(
                    px((visible.origin.x - frame.origin.x) as f32),
                    px(((frame.origin.y + frame.size.height)
                        - (visible.origin.y + visible.size.height)) as f32),
                ),
                size(
                    px(visible.size.width as f32),
                    px(visible.size.height as f32),
                ),
            ))
        })();
    Anchor { owner, work }
}

/// On X11 the owner's real place and the work area need the X server, which
/// is asked off the UI thread by [`place_later`]; here only what GPUI reports.
#[cfg(target_os = "linux")]
pub fn anchor(window: &Window, _title: &str) -> Anchor {
    Anchor {
        owner: window.bounds(),
        work: None,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn anchor(window: &Window, _title: &str) -> Anchor {
    Anchor {
        owner: window.bounds(),
        work: None,
    }
}

/// A logical rectangle as the physical pixels of a display with `scale`.
#[cfg(any(target_os = "windows", target_os = "linux", test))]
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
pub fn place_native(window: &Window, bounds: Bounds<Pixels>, scale: f32) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SetWindowPos,
    };

    if cfg!(test) {
        return;
    }
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

/// Moving a window on X11. A window manager decides where a new window goes
/// and ignores the position it was created with, so the place is computed from
/// the real positions and requested again (EWMH `_NET_MOVERESIZE_WINDOW`) once
/// the window manager lists the window. Wayland offers no way to place a
/// toplevel and is left to the compositor; a window manager without EWMH keeps
/// its own placement.
#[cfg(target_os = "linux")]
pub use x11::place_later;

#[cfg(target_os = "linux")]
mod x11 {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use gpui::{Bounds, Pixels, Size, point, px, size};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window as XWindow,
    };
    use x11rb::rust_connection::RustConnection;

    use super::physical_rect;
    use crate::window_layout::{intersection, monitor_of, settings_bounds, usable_area};

    /// How long a placement waits for the window manager to list the window.
    const PATIENCE: Duration = Duration::from_secs(3);

    /// The newest placement; an older one gives up when a window was opened
    /// again, so it cannot move the newer window.
    static LATEST: AtomicU64 = AtomicU64::new(0);

    struct Server {
        connection: RustConnection,
        root: XWindow,
        atoms: RefCell<HashMap<String, Option<Atom>>>,
    }

    impl Server {
        fn connect() -> Option<Self> {
            let (connection, screen) = RustConnection::connect(None).ok()?;
            let root = connection.setup().roots.get(screen)?.root;
            Some(Self {
                connection,
                root,
                atoms: RefCell::default(),
            })
        }

        fn atom(&self, name: &str) -> Option<Atom> {
            if let Some(known) = self.atoms.borrow().get(name) {
                return *known;
            }
            let atom = (|| {
                let atom = self
                    .connection
                    .intern_atom(true, name.as_bytes())
                    .ok()?
                    .reply()
                    .ok()?
                    .atom;
                (atom != 0).then_some(atom)
            })();
            self.atoms.borrow_mut().insert(name.to_owned(), atom);
            atom
        }

        fn cardinals(&self, window: XWindow, name: &str, length: u32) -> Option<Vec<u32>> {
            let reply = self
                .connection
                .get_property(false, window, self.atom(name)?, AtomEnum::ANY, 0, length)
                .ok()?
                .reply()
                .ok()?;
            Some(reply.value32()?.collect())
        }

        fn title(&self, window: XWindow) -> Option<String> {
            let reply = self
                .connection
                .get_property(
                    false,
                    window,
                    self.atom("_NET_WM_NAME")?,
                    AtomEnum::ANY,
                    0,
                    256,
                )
                .ok()?
                .reply()
                .ok()?;
            Some(String::from_utf8_lossy(&reply.value).into_owned())
        }

        /// This process's top-level windows the window manager manages, with
        /// their titles and content rectangles in physical root coordinates.
        fn ours(&self) -> Vec<(XWindow, String, [i32; 4])> {
            let pid = std::process::id();
            let list = self
                .cardinals(self.root, "_NET_CLIENT_LIST", 4096)
                .unwrap_or_default();
            list.into_iter()
                .filter(|&window| self.cardinals(window, "_NET_WM_PID", 1) == Some(vec![pid]))
                .filter_map(|window| {
                    let size = self.connection.get_geometry(window).ok()?.reply().ok()?;
                    let origin = self
                        .connection
                        .translate_coordinates(window, self.root, 0, 0)
                        .ok()?
                        .reply()
                        .ok()?;
                    Some((
                        window,
                        self.title(window).unwrap_or_default(),
                        [
                            origin.dst_x.into(),
                            origin.dst_y.into(),
                            size.width.into(),
                            size.height.into(),
                        ],
                    ))
                })
                .collect()
        }
    }

    /// Computes where the settings window goes and asks the window manager to
    /// put it there, on a thread of its own: the X server is asked
    /// synchronously and must never hold up the UI. `reported` is the owner as
    /// GPUI reports it (relative to the window manager's frame, so only used
    /// to recognise the owner among our windows), `title` the settings
    /// window's. Any failure leaves the window where it was created.
    pub fn place_later(reported: Bounds<Pixels>, scale: f32, title: String, wanted: Size<Pixels>) {
        if cfg!(test) {
            return;
        }
        let generation = LATEST.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = std::thread::Builder::new()
            .name("settings-placement".into())
            .spawn(move || place(reported, scale, &title, wanted, generation));
    }

    fn place(
        reported: Bounds<Pixels>,
        scale: f32,
        title: &str,
        wanted: Size<Pixels>,
        generation: u64,
    ) {
        let Some(server) = Server::connect() else {
            return;
        };
        let Some(message) = server.atom("_NET_MOVERESIZE_WINDOW") else {
            return;
        };
        let deadline = Instant::now() + PATIENCE;
        // At startup the owner itself may not be listed yet, so this retries too.
        let rect = loop {
            if let Some(bounds) = target_bounds(&server, reported, scale, title, wanted) {
                break physical_rect(bounds, scale).map(|value| value.max(0) as u32);
            }
            if LATEST.load(Ordering::SeqCst) != generation || Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        while LATEST.load(Ordering::SeqCst) == generation && Instant::now() < deadline {
            if let Some((window, ..)) = server.ours().into_iter().find(|(_, name, _)| name == title)
            {
                // Static gravity (10): x and y are the content's, not the
                // frame's. Bits 8-11 say x, y, width and height are given; 12
                // is "from an application".
                let flags = 10 | 1 << 8 | 1 << 9 | 1 << 10 | 1 << 11 | 1 << 12;
                let [x, y, width, height] = rect;
                let sent = server.connection.send_event(
                    false,
                    server.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    ClientMessageEvent::new(32, window, message, [flags, x, y, width, height]),
                );
                if sent.is_ok() {
                    let _ = server.connection.flush();
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The logical place for the settings window from the owner's real
    /// position and the work area of the monitor it is mostly on.
    fn target_bounds(
        server: &Server,
        reported: Bounds<Pixels>,
        scale: f32,
        settings_title: &str,
        wanted: Size<Pixels>,
    ) -> Option<Bounds<Pixels>> {
        let logical = |value: i32| px(value as f32 / scale);
        // The chat window: ours, not the settings window, the one whose
        // content size is the reported one (nearest to its reported place
        // when several match).
        let (_, _, [x, y, width, height]) = server
            .ours()
            .into_iter()
            .filter(|(_, title, [_, _, w, h])| {
                title != settings_title
                    && *w == (f32::from(reported.size.width) * scale).round() as i32
                    && *h == (f32::from(reported.size.height) * scale).round() as i32
            })
            .min_by(|a, b| {
                let off = |[x, y, ..]: [i32; 4]| {
                    f32::from((logical(x) - reported.origin.x).abs())
                        + f32::from((logical(y) - reported.origin.y).abs())
                };
                off(a.2).total_cmp(&off(b.2))
            })?;
        let owner = Bounds::new(
            point(logical(x), logical(y)),
            size(logical(width), logical(height)),
        );

        let monitors: Vec<Bounds<Pixels>> = server
            .connection
            .randr_get_monitors(server.root, true)
            .ok()?
            .reply()
            .ok()?
            .monitors
            .iter()
            .map(|monitor| {
                Bounds::new(
                    point(logical(monitor.x.into()), logical(monitor.y.into())),
                    size(
                        logical(monitor.width.into()),
                        logical(monitor.height.into()),
                    ),
                )
            })
            .collect();
        let monitor = monitor_of(owner, &monitors)?;

        // The window manager's work area of the current desktop (monitor
        // minus panels); it covers the whole screen, so it is cut to the
        // monitor. Without it the monitor itself is the area.
        let work = (|| {
            let desktop = *server
                .cardinals(server.root, "_NET_CURRENT_DESKTOP", 1)?
                .first()? as usize;
            let areas = server.cardinals(server.root, "_NET_WORKAREA", 4096)?;
            let area = areas.get(desktop * 4..desktop * 4 + 4)?;
            let area = Bounds::new(
                point(logical(area[0] as i32), logical(area[1] as i32)),
                size(logical(area[2] as i32), logical(area[3] as i32)),
            );
            intersection(monitor, area)
        })()
        .unwrap_or(monitor);
        Some(settings_bounds(owner, usable_area(work), wanted))
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

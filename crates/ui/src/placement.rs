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

/// On X11 GPUI gives no access to the native window, so the owner and the new
/// window are found among the window manager's client list by this process's
/// id. `title` is the settings window's, to tell it from the owner.
#[cfg(target_os = "linux")]
pub fn anchor(window: &Window, title: &str) -> Anchor {
    let owner = window.bounds();
    match x11::query(window, title) {
        Some((owner, work)) => Anchor {
            owner,
            work: Some(work),
        },
        None => Anchor { owner, work: None },
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn anchor(window: &Window, _title: &str) -> Anchor {
    Anchor {
        owner: window.bounds(),
        work: None,
    }
}

/// Moving a window on X11. A window manager decides where a new window goes
/// and ignores the position it was created with, so the position is
/// requested again (EWMH `_NET_MOVERESIZE_WINDOW`) once the window manager
/// lists the window. Wayland offers no way to place a toplevel and is left to
/// the compositor; a window manager without EWMH keeps its own placement.
#[cfg(target_os = "linux")]
pub use x11::{move_target, try_move};

#[cfg(target_os = "linux")]
mod x11 {
    use gpui::{Bounds, Pixels, Window, point, px, size};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window as XWindow,
    };
    use x11rb::rust_connection::RustConnection;

    use crate::window_layout::{intersection, monitor_of};

    struct Server {
        connection: RustConnection,
        root: XWindow,
    }

    impl Server {
        fn connect() -> Option<Self> {
            let (connection, screen) = RustConnection::connect(None).ok()?;
            let root = connection.setup().roots.get(screen)?.root;
            Some(Self { connection, root })
        }

        fn atom(&self, name: &str) -> Option<Atom> {
            let atom = self
                .connection
                .intern_atom(true, name.as_bytes())
                .ok()?
                .reply()
                .ok()?
                .atom;
            (atom != 0).then_some(atom)
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

    /// The owner's real position on the X screen (its own events give it
    /// relative to the window manager's frame) and the work area of the
    /// monitor it is mostly on, in logical pixels. `None` when not on X11 or
    /// the window manager does not list windows.
    pub fn query(
        window: &Window,
        settings_title: &str,
    ) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        let server = Server::connect()?;
        let scale = window.scale_factor();
        let logical = |value: i32| px(value as f32 / scale);
        let wanted = window.bounds();
        // The chat window: ours, not the settings window, the one whose
        // content size is the reported one (nearest to its reported place
        // when several match).
        let (_, _, [x, y, width, height]) = server
            .ours()
            .into_iter()
            .filter(|(_, title, [_, _, w, h])| {
                title != settings_title
                    && *w == (f32::from(wanted.size.width) * scale).round() as i32
                    && *h == (f32::from(wanted.size.height) * scale).round() as i32
            })
            .min_by(|a, b| {
                let off = |[x, y, ..]: [i32; 4]| {
                    f32::from((logical(x) - wanted.origin.x).abs())
                        + f32::from((logical(y) - wanted.origin.y).abs())
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
        Some((owner, work))
    }

    /// A window to move: its title and the wanted content rectangle in
    /// physical pixels.
    pub struct MoveTarget {
        title: String,
        rect: [u32; 4],
    }

    pub fn move_target(window: &Window, title: &str, bounds: Bounds<Pixels>) -> MoveTarget {
        let scale = window.scale_factor();
        let physical = |value: Pixels| (f32::from(value) * scale).round() as i32 as u32;
        MoveTarget {
            title: title.to_owned(),
            rect: [
                physical(bounds.origin.x),
                physical(bounds.origin.y),
                physical(bounds.size.width),
                physical(bounds.size.height),
            ],
        }
    }

    /// Asks the window manager to put the window where wanted once it lists
    /// it. `Some(true)` when asked, `Some(false)` when to try again later,
    /// `None` when it cannot be done at all.
    pub fn try_move(target: &MoveTarget) -> Option<bool> {
        let server = Server::connect()?;
        let message = server.atom("_NET_MOVERESIZE_WINDOW")?;
        let Some((window, ..)) = server
            .ours()
            .into_iter()
            .find(|(_, title, _)| *title == target.title)
        else {
            return Some(false);
        };
        // Static gravity (10): x and y are the content's, not the frame's.
        // Bits 8-11 say x, y, width and height are given; 12 is "from an
        // application".
        let flags = 10 | 1 << 8 | 1 << 9 | 1 << 10 | 1 << 11 | 1 << 12;
        let [x, y, width, height] = target.rect;
        server
            .connection
            .send_event(
                false,
                server.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                ClientMessageEvent::new(32, window, message, [flags, x, y, width, height]),
            )
            .ok()?;
        server.connection.flush().ok()?;
        Some(true)
    }
}

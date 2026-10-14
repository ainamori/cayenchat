// SPDX-License-Identifier: GPL-3.0-only
//! Reopening the main window where it was left.
//!
//! Only what GPUI cannot know lives here: whether a saved rectangle still
//! suits the displays that are connected now. Displays come and go (a laptop
//! undocked, a monitor unplugged, a resolution change), so a saved position is
//! used only where it still overlaps a display, and is moved and shrunk to fit
//! that display; otherwise the caller opens the window in its usual place.

use cayenchat_storage::layout::Layout;
use gpui::{Bounds, Pixels, Size, WindowBounds, point, px, size};

/// Where to open the window again.
#[derive(Debug)]
pub struct Restored {
    pub bounds: WindowBounds,
    /// Index into the displays given of the one the window goes on.
    pub display: usize,
}

/// The bounds to open the window with and their display, or `None` when
/// nothing is saved or no connected display shows any of the saved rectangle.
pub fn restored_bounds(
    layout: &Layout,
    displays: &[Bounds<Pixels>],
    min: Size<Pixels>,
) -> Option<Restored> {
    let saved = layout.window?;
    let (min_width, min_height) = (f32::from(min.width), f32::from(min.height));
    // The display showing most of the saved rectangle.
    let overlap = |display: &Bounds<Pixels>| {
        let (left, top) = (f32::from(display.left()), f32::from(display.top()));
        let (right, bottom) = (f32::from(display.right()), f32::from(display.bottom()));
        let width = (saved.x + saved.width).min(right) - saved.x.max(left);
        let height = (saved.y + saved.height).min(bottom) - saved.y.max(top);
        if width > 0. && height > 0. {
            width * height
        } else {
            0.
        }
    };
    let (index, display) = displays
        .iter()
        .enumerate()
        .map(|(index, display)| (overlap(display), index, display))
        .filter(|(area, _, _)| *area > 0.)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index, display)| (index, *display))?;

    let (left, top) = (f32::from(display.left()), f32::from(display.top()));
    let (screen_width, screen_height) = (
        f32::from(display.size.width),
        f32::from(display.size.height),
    );
    // Not smaller than the window allows, not larger than the display.
    let width = saved.width.min(screen_width).max(min_width);
    let height = saved.height.min(screen_height).max(min_height);
    // Moved onto the display as far as it fits (a window larger than the
    // display, only possible at the minimum size, keeps its top left).
    let x = saved.x.min(left + screen_width - width).max(left);
    let y = saved.y.min(top + screen_height - height).max(top);
    let bounds = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
    Some(Restored {
        bounds: if layout.maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        },
        display: index,
    })
}

/// The display area a new window's content may use: the work area where the
/// platform tells it (excluding taskbar and Dock), otherwise `display` less a
/// fixed reserve for a menu bar, taskbar or Dock. Either way room is left for
/// the title bar and frame, which GPUI's window bounds do not include.
pub fn usable_area(display: Bounds<Pixels>, work: Option<Bounds<Pixels>>) -> Bounds<Pixels> {
    const TITLE: f32 = 40.;
    const FRAME: f32 = 8.;
    const RESERVE_TOP: f32 = 32.;
    const RESERVE_BOTTOM: f32 = 80.;
    let base = work.unwrap_or_else(|| {
        let height = f32::from(display.size.height) - RESERVE_TOP - RESERVE_BOTTOM;
        Bounds::new(
            point(display.origin.x, display.origin.y + px(RESERVE_TOP)),
            size(display.size.width, px(height.max(1.))),
        )
    });
    let width = f32::from(base.size.width) - 2. * FRAME;
    let height = f32::from(base.size.height) - TITLE - FRAME;
    Bounds::new(
        point(base.origin.x + px(FRAME), base.origin.y + px(TITLE)),
        size(px(width.max(1.)), px(height.max(1.))),
    )
}

/// Where a new settings window opens: horizontally centered on the owner
/// (the chat window). Vertically it either starts at the owner's middle or
/// ends there, whichever leaves more of the owner visible (the first when
/// equal). The whole window stays inside `area` (see [`usable_area`]), which
/// wins over both; it is shrunk when `area` is smaller than `wanted`, even
/// below the window's usual minimum, so the caller lowers the minimum to the
/// returned size.
pub fn settings_bounds(
    owner: Bounds<Pixels>,
    area: Bounds<Pixels>,
    wanted: Size<Pixels>,
) -> Bounds<Pixels> {
    let (left, top) = (f32::from(area.left()), f32::from(area.top()));
    let (area_width, area_height) = (f32::from(area.size.width), f32::from(area.size.height));
    let width = f32::from(wanted.width).min(area_width);
    let height = f32::from(wanted.height).min(area_height);
    let x = (f32::from(owner.center().x) - width / 2.)
        .min(left + area_width - width)
        .max(left);
    let middle = f32::from(owner.center().y);
    let fit = |y: f32| y.min(top + area_height - height).max(top);
    // How much of the owner a settings window at `y` leaves uncovered.
    let visible = |y: f32| {
        let across = (x + width).min(f32::from(owner.right())) - x.max(f32::from(owner.left()));
        let down = (y + height).min(f32::from(owner.bottom())) - y.max(f32::from(owner.top()));
        let covered = if across > 0. && down > 0. {
            across * down
        } else {
            0.
        };
        f32::from(owner.size.width) * f32::from(owner.size.height) - covered
    };
    let below = fit(middle);
    let above = fit(middle - height);
    let y = if visible(above) > visible(below) {
        above
    } else {
        below
    };
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cayenchat_storage::layout::WindowRect;

    fn rect(bounds: Bounds<Pixels>) -> (f32, f32, f32, f32) {
        (
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    }

    #[test]
    fn settings_are_centered_horizontally_on_the_owner() {
        let screen = display(0., 0., 1920., 1080.);
        let owner = display(400., 100., 960., 500.);
        let got = settings_bounds(owner, screen, size(px(900.), px(400.)));
        // Starts at the owner's middle (350), leaving its upper part visible.
        assert_eq!(rect(got), (430., 350., 900., 400.));
    }

    #[test]
    fn settings_go_above_when_below_would_hide_the_owner() {
        let screen = display(0., 0., 1920., 1080.);
        let owner = display(400., 500., 960., 500.);
        let got = settings_bounds(owner, screen, size(px(900.), px(750.)));
        // Below the middle it would be pushed up over the whole owner; ending
        // at the screen top instead leaves the owner's lower half.
        assert_eq!(rect(got), (430., 0., 900., 750.));
    }

    #[test]
    fn usable_area_leaves_room_for_the_frame_and_the_work_area() {
        let screen = display(0., 0., 1920., 1080.);
        let work = display(0., 0., 1920., 1032.);
        assert_eq!(
            rect(usable_area(screen, Some(work))),
            (8., 40., 1904., 984.)
        );
        // Work area unknown: a menu bar and a taskbar or Dock are assumed.
        assert_eq!(rect(usable_area(screen, None)), (8., 72., 1904., 920.));
    }

    #[test]
    fn settings_stay_on_the_display_the_owner_is_on() {
        let screens = display(-1280., 0., 1280., 1024.);
        // Owner at the left edge of a monitor with negative coordinates.
        let owner = display(-1280., 900., 800., 124.);
        let got = settings_bounds(owner, screens, size(px(900.), px(750.)));
        assert_eq!(rect(got), (-1280., 212., 900., 750.));
        // At the bottom right corner.
        let owner = display(-500., 800., 500., 224.);
        let got = settings_bounds(owner, screens, size(px(900.), px(750.)));
        assert_eq!(rect(got), (-900., 162., 900., 750.));
    }

    #[test]
    fn settings_shrink_to_a_small_area_even_below_the_usual_minimum() {
        let owner = display(0., 0., 700., 500.);
        let small = display(0., 0., 800., 600.);
        let got = settings_bounds(owner, small, size(px(900.), px(750.)));
        assert_eq!(rect(got), (0., 0., 800., 600.));
        let tiny = display(10., 20., 600., 400.);
        let got = settings_bounds(owner, tiny, size(px(900.), px(750.)));
        assert_eq!(rect(got), (10., 20., 600., 400.));
    }

    const MIN: fn() -> Size<Pixels> = || size(px(320.), px(240.));

    fn display(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn saved(x: f32, y: f32, width: f32, height: f32, maximized: bool) -> Layout {
        Layout {
            window: Some(WindowRect {
                x,
                y,
                width,
                height,
            }),
            maximized,
            ..Layout::default()
        }
    }

    fn windowed(restored: Option<Restored>) -> (f32, f32, f32, f32) {
        match restored.map(|restored| restored.bounds) {
            Some(WindowBounds::Windowed(b)) | Some(WindowBounds::Maximized(b)) => (
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            ),
            other => panic!("expected bounds, got {other:?}"),
        }
    }

    #[test]
    fn a_rectangle_that_fits_comes_back_unchanged() {
        let screens = [display(0., 0., 1920., 1080.)];
        let got = restored_bounds(&saved(100., 80., 960., 600., false), &screens, MIN());
        assert!(matches!(
            got,
            Some(Restored {
                bounds: WindowBounds::Windowed(_),
                display: 0
            })
        ));
        assert_eq!(windowed(got), (100., 80., 960., 600.));
    }

    #[test]
    fn nothing_saved_or_no_display_overlapping_means_the_usual_place() {
        let screens = [display(0., 0., 1920., 1080.)];
        assert!(restored_bounds(&Layout::default(), &screens, MIN()).is_none());
        // The monitor it was on is gone: it was at x 2000.. beside this one.
        assert!(restored_bounds(&saved(2100., 100., 960., 600., false), &screens, MIN()).is_none());
        // Touching an edge shows nothing.
        assert!(restored_bounds(&saved(1920., 0., 960., 600., false), &screens, MIN()).is_none());
        assert!(restored_bounds(&saved(0., 0., 960., 600., false), &[], MIN()).is_none());
    }

    #[test]
    fn a_window_partly_off_screen_is_moved_back_onto_it() {
        let screens = [display(0., 0., 1920., 1080.)];
        let got = restored_bounds(&saved(1500., 800., 960., 600., false), &screens, MIN());
        assert_eq!(windowed(got), (960., 480., 960., 600.));
        let got = restored_bounds(&saved(-300., -50., 960., 600., false), &screens, MIN());
        assert_eq!(windowed(got), (0., 0., 960., 600.));
    }

    #[test]
    fn the_size_is_kept_between_the_minimum_and_the_display() {
        let screens = [display(0., 0., 1280., 800.)];
        let got = restored_bounds(&saved(0., 0., 3000., 2000., false), &screens, MIN());
        assert_eq!(windowed(got), (0., 0., 1280., 800.));
        let got = restored_bounds(&saved(100., 100., 200., 100., false), &screens, MIN());
        assert_eq!(windowed(got), (100., 100., 320., 240.));
        // A small size the user chose is not enlarged.
        let got = restored_bounds(&saved(100., 100., 400., 300., false), &screens, MIN());
        assert_eq!(windowed(got), (100., 100., 400., 300.));
        // A display smaller than the minimum keeps the window's top left.
        let tiny = [display(0., 0., 300., 200.)];
        let got = restored_bounds(&saved(10., 10., 960., 600., false), &tiny, MIN());
        assert_eq!(windowed(got), (0., 0., 320., 240.));
    }

    #[test]
    fn with_several_displays_the_one_showing_most_of_it_decides() {
        // A second monitor to the left (negative x) and a primary to the right.
        let screens = [
            display(0., 0., 1920., 1080.),
            display(-1280., 0., 1280., 1024.),
        ];
        // Mostly on the left monitor, which is named so that the window opens
        // there rather than on the primary.
        let got = restored_bounds(&saved(-1100., 100., 960., 600., false), &screens, MIN());
        assert_eq!(got.as_ref().map(|restored| restored.display), Some(1));
        assert_eq!(windowed(got), (-1100., 100., 960., 600.));
        // Straddling, mostly on the primary: it stays within the primary.
        let got = restored_bounds(&saved(-200., 100., 960., 600., false), &screens, MIN());
        assert_eq!(got.as_ref().map(|restored| restored.display), Some(0));
        assert_eq!(windowed(got), (0., 100., 960., 600.));
        // With the left monitor gone it is brought to the primary.
        let got = restored_bounds(
            &saved(-1100., 100., 960., 600., false),
            &screens[..1],
            MIN(),
        );
        assert!(got.is_none(), "no overlap with the remaining display");
        let got = restored_bounds(&saved(-600., 100., 960., 600., false), &screens[..1], MIN());
        assert_eq!(windowed(got), (0., 100., 960., 600.));
    }

    #[test]
    fn three_displays_with_one_offset_keep_the_window_where_it_was() {
        // Primary in the middle, one to the right raised by 300 and one to
        // the left (#125).
        let screens = [
            display(0., 0., 1920., 1080.),
            display(1920., -300., 2560., 1440.),
            display(-1920., 0., 1920., 1080.),
        ];
        let got = restored_bounds(&saved(2200., -250., 1200., 800., false), &screens, MIN());
        assert_eq!(got.as_ref().map(|restored| restored.display), Some(1));
        assert_eq!(windowed(got), (2200., -250., 1200., 800.));
        let got = restored_bounds(&saved(-1500., 300., 1000., 700., true), &screens, MIN());
        assert_eq!(got.as_ref().map(|restored| restored.display), Some(2));
        assert_eq!(windowed(got), (-1500., 300., 1000., 700.));
    }

    #[test]
    fn maximized_keeps_the_restore_rectangle() {
        let screens = [display(0., 0., 1920., 1080.)];
        let got = restored_bounds(&saved(200., 100., 960., 600., true), &screens, MIN());
        assert!(matches!(
            got,
            Some(Restored {
                bounds: WindowBounds::Maximized(_),
                ..
            })
        ));
        assert_eq!(windowed(got), (200., 100., 960., 600.));
    }
}

// The window's zoom, a port of TextZoom and TypeScale.scaled in Theme.swift:
// ⌘= and ⌘- step through `STEPS`, ⌘0 puts it back, and everything scales
// together, like a browser's zoom.

/// `TextZoom.steps`: percentages of the design sizes.
pub const STEPS: [u32; 9] = [80, 90, 100, 110, 125, 140, 160, 180, 200];
pub const DEFAULT_PERCENT: u32 = 100;

/// The saved percent, or 100 for anything that isn't one of the steps.
pub fn sanitize(saved: Option<u32>) -> u32 {
    saved.filter(|p| STEPS.contains(p)).unwrap_or(DEFAULT_PERCENT)
}

pub fn can_zoom_in(percent: u32) -> bool {
    percent < *STEPS.last().unwrap()
}

pub fn can_zoom_out(percent: u32) -> bool {
    percent > STEPS[0]
}

pub fn zoom_in(percent: u32) -> u32 {
    STEPS.iter().copied().find(|s| *s > percent).unwrap_or(percent)
}

pub fn zoom_out(percent: u32) -> u32 {
    STEPS.iter().rev().copied().find(|s| *s < percent).unwrap_or(percent)
}

/// `size` at this zoom: to a whole point, except at 100% where the design sizes
/// stand as written, half points and all (`TypeScale.scaled`). Half-point steps
/// made neighbouring text almost, but not quite, the same size.
pub fn scaled(size: f32, percent: u32) -> f32 {
    if percent == 100 {
        size
    } else {
        (size * percent as f32 / 100.).round()
    }
}

/// A window's frame or a screen, in points with the origin at the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Mostly on a screen that's connected now: a frame left on a display that's
/// gone would open the window where it can't be seen (`MainWindowFrame.onScreen`).
pub fn on_screen(frame: Rect, screens: &[Rect]) -> bool {
    if frame.width < 100. || frame.height < 100. {
        return false;
    }
    let area = frame.width * frame.height;
    screens.iter().any(|s| {
        let w = (frame.x + frame.width).min(s.x + s.width) - frame.x.max(s.x);
        let h = (frame.y + frame.height).min(s.y + s.height) - frame.y.max(s.y);
        w > 0. && h > 0. && w * h >= area / 2.
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_steps_through_the_swift_app_steps() {
        assert_eq!(zoom_in(100), 110);
        assert_eq!(zoom_in(200), 200, "the last step stays");
        assert_eq!(zoom_out(100), 90);
        assert_eq!(zoom_out(80), 80, "the first step stays");
        assert_eq!(zoom_in(115), 125, "from between the steps, the next one");
    }

    #[test]
    fn a_saved_percent_must_be_a_step() {
        assert_eq!(sanitize(Some(125)), 125);
        assert_eq!(sanitize(Some(123)), 100);
        assert_eq!(sanitize(None), 100);
    }

    #[test]
    fn sizes_scale_to_whole_points() {
        assert_eq!(scaled(13.5, 100), 13.5, "the design sizes stand at 100%");
        assert_eq!(scaled(15., 125), 19.);
        assert_eq!(scaled(10., 110), 11.);
    }

    #[test]
    fn a_frame_mostly_off_every_screen_isnt_used() {
        let screen = Rect { x: 0., y: 0., width: 1600., height: 1000. };
        let frame = |x: f64, y: f64| Rect { x, y, width: 1000., height: 800. };
        assert!(on_screen(frame(100., 50.), &[screen]));
        assert!(!on_screen(frame(3000., 0.), &[screen]), "on a display that's gone");
        assert!(!on_screen(frame(1500., 0.), &[screen]), "under half on the screen");
        assert!(!on_screen(Rect { x: 0., y: 0., width: 50., height: 50. }, &[screen]), "too small to be a real frame");
    }
}

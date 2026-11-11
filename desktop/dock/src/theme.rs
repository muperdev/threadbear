// The dock's colours, read from the app's gpui-kit theme so the dock follows
// light and dark like everything around it (Swift's DockTheme used the system
// colours for the same reason).
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

#[derive(Clone, Copy)]
pub struct DockTheme {
    pub panel: Hsla,
    /// Shows through the 1 pt gaps between panels, as the hairlines.
    pub hairline: Hsla,
    pub header_text: Hsla,
    pub secondary: Hsla,
    pub faint: Hsla,
    pub accent: Hsla,
    /// The line under a bar: `Color.primary.opacity(0.07)`.
    pub bar_line: Hsla,
    /// Selected tab fill: primary 7%, hover 4%.
    pub tab_selected: Hsla,
    pub tab_hover: Hsla,
    /// The compass's plate and the ghost's outline.
    pub plate: Hsla,
    pub plate_border: Hsla,
    pub ghost_border: Hsla,
    pub shadow: Hsla,
    pub is_dark: bool,
}

/// The gap between panels: the hairlines are what shows through it.
pub const GAP: f32 = 1.;

impl DockTheme {
    pub fn new(cx: &App, accent: Option<Hsla>) -> Self {
        let theme = cx.theme();
        let is_dark = theme.is_dark();
        // `Color.primary`, as the bars and tabs use it for tints.
        let primary = theme.foreground;
        DockTheme {
            panel: theme.background,
            hairline: if is_dark { hsla(0., 0., 1., 0.13) } else { hsla(0., 0., 0., 0.13) },
            header_text: theme.foreground,
            secondary: theme.muted_foreground,
            faint: if is_dark { hsla(0., 0., 1., 0.3) } else { hsla(0., 0., 0., 0.28) },
            accent: accent.unwrap_or(theme.primary),
            bar_line: primary.opacity(0.07),
            tab_selected: primary.opacity(0.07),
            tab_hover: primary.opacity(0.04),
            plate: if is_dark { hsla(0., 0., 0.15, 0.92) } else { hsla(0., 0., 1., 0.92) },
            plate_border: primary.opacity(0.1),
            ghost_border: primary.opacity(0.14),
            shadow: hsla(0., 0., 0., 0.5),
            is_dark,
        }
    }
}

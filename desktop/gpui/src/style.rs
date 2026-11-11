// The Swift app's sizes and colours (Transcript/Theme.swift, TypeScale), so the
// two apps look alike and there's one place to change them. Colours take the
// light or dark value from the window's theme.
#![allow(dead_code)] // Filled in ahead of the views that use each value.
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use std::sync::atomic::{AtomicU32, Ordering};

// The window's zoom (TextZoom in Theme.swift): ⌘= and ⌘- scale everything
// together, like a browser's zoom. Every point size in the window goes through
// `px` below, so one number scales them all. A module that draws anything
// does `use crate::style::px;` after its `use gpui_kit::*;`, which shadows
// GPUI's own `px` for that file (a one-line change in each) and leaves a
// raw `gpui_kit::px` only for the rare size that shouldn't scale, like a
// hairline.
static ZOOM_PERCENT: AtomicU32 = AtomicU32::new(100);

pub fn zoom_percent() -> u32 {
    ZOOM_PERCENT.load(Ordering::Relaxed)
}

pub fn set_zoom_percent(percent: u32) {
    ZOOM_PERCENT.store(percent, Ordering::Relaxed);
}

/// `zoomed`: a design size at the window's zoom (`TypeScale.scaled`).
pub fn zoomed(size: f32) -> f32 {
    threadbear_core::zoom::scaled(size, zoom_percent())
}

/// GPUI's `px`, at the window's zoom.
pub fn px(size: f32) -> Pixels {
    gpui_kit::px(zoomed(size))
}

/// Text sizes, as TypeScale has them at 100% zoom.
pub mod size {
    pub const BODY: f32 = 15.;
    pub const BODY_LINE: f32 = 23.;
    pub const WORK: f32 = 14.;
    pub const WORK_LINE: f32 = 20.;
    pub const CODE: f32 = 13.;
    pub const CODE_LINE: f32 = 19.5;
    pub const CARD_TITLE: f32 = 14.;
    pub const LABEL: f32 = 13.;
    pub const DETAIL: f32 = 12.;
    pub const SMALL: f32 = 11.;
    pub const SIDEBAR_TITLE: f32 = 14.;
    pub const SIDEBAR_PREVIEW: f32 = 13.;
    pub const SIDEBAR_BUTTON: f32 = 14.;
    pub const SIDEBAR_META: f32 = 11.;
    pub const SIDEBAR_HEADING: f32 = 13.;
    pub const HEADER: f32 = 15.;
    pub const COMPOSER: f32 = 15.;
    pub const CHIP: f32 = 13.;
    /// The send button, and the height of the controls row it sits in.
    pub const SEND_BUTTON: f32 = 28.;
    /// A picture's chip among the words (PictureChip.Metrics at 15 pt).
    pub const CHIP_HEIGHT: f32 = 20.;
    /// A picture in the strip over the words (AttachmentThumbnail.size).
    pub const THUMBNAIL: f32 = 56.;
}

/// Widest the reading column gets; wider windows centre it.
pub const MAX_CONTENT_WIDTH: f32 = 780.;
pub const SIDE_INSET: f32 = 28.;

/// The composer's card (ComposerView).
pub const COMPOSER_RADIUS: f32 = 20.;
pub const COMPOSER_PADDING: f32 = 14.;
pub const COMPOSER_GAP: f32 = 10.;

/// The window's background in each look, as the Swift app draws its panels,
/// sidebar and transcript (NSColor.windowBackgroundColor, BearDock's
/// `DockTheme.panel`). gpui-kit's own dark theme is a near-black #0a0a0a, so the
/// whole GPUI app looked much darker than the Swift one (10 Nov). Put into
/// gpui-kit's light and dark themes before the first switch to either, so
/// every switch after (the system's, the automation's `appearance`) keeps it.
/// Popovers and menus get it lifted a little, as macOS's own menus are on dark.
pub(crate) fn install_window_colours(cx: &mut App) {
    use gpui_kit::component::Theme;
    use std::rc::Rc;
    let (light, dark) = window_backgrounds();
    let hex = |c: Rgba| -> SharedString { format!("#{:02x}{:02x}{:02x}", (c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8).into() };
    let lift = |c: Rgba, by: f32| Rgba { r: c.r + (1. - c.r) * by, g: c.g + (1. - c.g) * by, b: c.b + (1. - c.b) * by, a: 1. };
    let theme = Theme::global_mut(cx);
    for (config, background, popover) in [(&mut theme.light_theme, light, light), (&mut theme.dark_theme, dark, lift(dark, 0.06))] {
        let mut changed = (**config).clone();
        changed.colors.background = Some(hex(background));
        changed.colors.sidebar = Some(hex(background));
        changed.colors.popover = Some(hex(popover));
        *config = Rc::new(changed);
    }
}

/// NSColor.windowBackgroundColor in the light and dark looks. On the Mac it's
/// read from AppKit, so it follows whatever this macOS draws; elsewhere it's
/// what macOS 26 and 27 give, so the app looks the same everywhere.
fn window_backgrounds() -> (Rgba, Rgba) {
    let fallback = (srgb_rgba(1., 1., 1.), srgb_rgba(30. / 255., 30. / 255., 30. / 255.));
    #[cfg(target_os = "macos")]
    {
        let light = system_window_background(false).unwrap_or(fallback.0);
        let dark = system_window_background(true).unwrap_or(fallback.1);
        (light, dark)
    }
    #[cfg(not(target_os = "macos"))]
    fallback
}

fn srgb_rgba(r: f32, g: f32, b: f32) -> Rgba {
    Rgba { r, g, b, a: 1. }
}

// A dynamic colour resolves for the current appearance. The block form
// (performAsCurrentDrawingAppearance) needs block2; setting it is deprecated
// but does the same for a moment on the main thread.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn system_window_background(dark: bool) -> Option<Rgba> {
    use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace};
    unsafe {
        let name = if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua };
        let appearance = NSAppearance::appearanceNamed(name)?;
        let previous = NSAppearance::currentAppearance();
        NSAppearance::setCurrentAppearance(Some(&appearance));
        let color = NSColor::windowBackgroundColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace());
        NSAppearance::setCurrentAppearance(previous.as_deref());
        let c = color?;
        Some(srgb_rgba(c.redComponent() as f32, c.greenComponent() as f32, c.blueComponent() as f32))
    }
}

fn pick(cx: &App, dark: Hsla, light: Hsla) -> Hsla {
    if cx.theme().is_dark() {
        dark
    } else {
        light
    }
}

fn white(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}

fn black(alpha: f32) -> Hsla {
    hsla(0., 0., 0., alpha)
}

pub fn card_background(cx: &App) -> Hsla {
    pick(cx, white(0.035), black(0.02))
}

pub fn card_border(cx: &App) -> Hsla {
    pick(cx, white(0.12), black(0.12))
}

pub fn code_background(cx: &App) -> Hsla {
    pick(cx, white(0.045), black(0.03))
}

pub fn inline_code_background(cx: &App) -> Hsla {
    pick(cx, white(0.085), black(0.05))
}

pub fn hover_fill(cx: &App) -> Hsla {
    pick(cx, white(0.05), black(0.04))
}

pub fn hairline(cx: &App) -> Hsla {
    pick(cx, white(0.1), black(0.1))
}

pub fn faint_text(cx: &App) -> Hsla {
    pick(cx, white(0.36), black(0.45))
}

pub fn quiet_text(cx: &App) -> Hsla {
    pick(cx, white(0.55), black(0.6))
}

pub fn button_fill(cx: &App) -> Hsla {
    pick(cx, white(0.12), white(1.))
}

pub fn button_edge(cx: &App) -> Hsla {
    pick(cx, white(0.08), black(0.18))
}

/// The composer's card: a lighter fill on dark, white on light (ComposerView.card).
pub fn composer_fill(cx: &App) -> Hsla {
    pick(cx, white(0.085), white(1.))
}

/// The empty look of the send button (CircleButtonStyle, not filled).
pub fn send_faint_fill(cx: &App) -> Hsla {
    pick(cx, white(0.1), black(0.1))
}

/// A picture chip's fill (PictureChip.fill).
pub fn chip_fill(cx: &App) -> Hsla {
    pick(cx, white(0.1), black(0.06))
}

/// A composer chip's hover fill (ComposerChip).
pub fn chip_hover_fill(cx: &App) -> Hsla {
    pick(cx, white(0.07), black(0.05))
}

/// The hairline between chips (ChipDivider).
pub fn chip_divider(cx: &App) -> Hsla {
    pick(cx, white(0.12), black(0.12))
}

/// The Needs you orange (Theme.needsYou).
pub fn needs_you(cx: &App) -> Hsla {
    pick(cx, hsla(0.09, 0.95, 0.56, 1.), hsla(0.09, 0.9, 0.45, 1.))
}

/// The answer box under an ask's Something else.
pub fn answer_box_fill(cx: &App) -> Hsla {
    pick(cx, white(0.05), black(0.03))
}

/// The system's accent (NSColor.controlAccentColor): the outline of a box
/// that needs Mike, ticks and the agent's own pick.
pub fn accent(cx: &App) -> Hsla {
    pick(cx, rgb_hsla(0x0a84ff), rgb_hsla(0x007aff))
}

fn rgb_hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

// The transcript's colours beyond the cards' (Theme.swift).

/// `color` drawn over the window's background, for text runs: GPUI blends a
/// highlight's colour into the run's default colour, so a translucent one
/// (every transcript colour here) would come out as the default colour.
pub fn over_background(color: Hsla, cx: &App) -> Hsla {
    let (bg, c) = (Rgba::from(cx.theme().background), Rgba::from(color));
    Hsla::from(Rgba { r: bg.r * (1. - c.a) + c.r * c.a, g: bg.g * (1. - c.a) + c.g * c.a, b: bg.b * (1. - c.a) + c.b * c.a, a: 1. })
}

/// NSColor.labelColor: body text.
pub fn label_text(cx: &App) -> Hsla {
    pick(cx, white(0.85), black(0.85))
}

/// NSColor.secondaryLabelColor: a table's headers, quoted text.
pub fn secondary_label(cx: &App) -> Hsla {
    pick(cx, white(0.55), black(0.5))
}

/// NSColor.tertiaryLabelColor, and `Theme.faintIcon`'s dark value: work line
/// icons and chevrons, the "⏎" on the Allow button.
pub fn tertiary_label(cx: &App) -> Hsla {
    pick(cx, white(0.25), black(0.25))
}

/// `Theme.faintIcon`: work line icons and chevrons.
pub fn faint_icon(cx: &App) -> Hsla {
    pick(cx, white(0.25), black(0.36))
}

/// The pane's row icons (`Theme.faintIcon`): a notch brighter here, as
/// Lucide's thin strokes read fainter than SF Symbols.
pub fn pane_icon(cx: &App) -> Hsla {
    pick(cx, white(0.4), black(0.42))
}

/// `Theme.answerRowsFill`.
pub fn answer_rows_fill(cx: &App) -> Hsla {
    pick(cx, white(0.02), black(0.015))
}

/// The small star beside "Recommended" on the agent's own pick: a muted gold,
/// as on the Mac app (Theme.recommendedStar), after Ghostex's (Mike, 9 Nov 2026).
pub fn recommended_star(cx: &App) -> Hsla {
    pick(cx, srgb(0.86, 0.70, 0.33, 1.), srgb(0.72, 0.54, 0.12, 1.))
}

fn srgb(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    Hsla::from(Rgba { r, g, b, a })
}

/// macOS's default blue, as NSColor.controlAccentColor is for most people. The
/// app doesn't follow the system's accent yet (foundation.theme).
pub fn control_accent(cx: &App) -> Hsla {
    pick(cx, srgb(0.04, 0.52, 1.0, 1.0), srgb(0.0, 0.48, 1.0, 1.0))
}

/// NSColor.linkColor.
pub fn link_text(cx: &App) -> Hsla {
    pick(cx, srgb(0.04, 0.52, 1.0, 1.0), srgb(0.0, 0.40, 0.8, 1.0))
}

/// A little of the accent over the faint fill behind Mike's own messages.
pub fn bubble_background(cx: &App) -> Hsla {
    let dark = cx.theme().is_dark();
    let accent = Rgba::from(control_accent(cx));
    let (base, base_alpha, accent_alpha): (f32, f32, f32) = if dark { (1., 0.06, 0.09) } else { (0., 0.035, 0.09) };
    let alpha = accent_alpha + base_alpha * (1. - accent_alpha);
    let mix = |c: f32| (c * accent_alpha + base * base_alpha * (1. - accent_alpha)) / alpha;
    srgb(mix(accent.r), mix(accent.g), mix(accent.b), alpha)
}

/// A faint line's tint in a diff: green for added lines, red for removed.
pub fn added_line_bg(cx: &App) -> Hsla {
    pick(cx, srgb(0.25, 0.78, 0.4, 0.12), srgb(0.1, 0.62, 0.25, 0.1))
}

pub fn removed_line_bg(cx: &App) -> Hsla {
    pick(cx, srgb(0.95, 0.3, 0.3, 0.12), srgb(0.85, 0.2, 0.2, 0.09))
}

pub fn added_text(cx: &App) -> Hsla {
    pick(cx, srgb(0.42, 0.84, 0.5, 1.), srgb(0.1, 0.55, 0.22, 1.))
}

pub fn removed_text(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.45, 0.45, 1.), srgb(0.8, 0.15, 0.15, 1.))
}

/// `Theme.failed`: "Failed" and a failed tool's icon.
pub fn failed_text(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.27, 0.23, 1.), srgb(0.78, 0.13, 0.13, 1.))
}

/// `Theme.needsYou` and NSColor.systemOrange.
pub fn needs_you_text(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.62, 0.04, 1.), srgb(0.7, 0.33, 0., 1.))
}

/// `Theme.errorBackground`: behind a failed turn's message.
pub fn error_background(cx: &App) -> Hsla {
    pick(cx, srgb(0.7, 0.2, 0.2, 0.18), srgb(1., 0.94, 0.94, 1.))
}

// The sidebar's colours (SidebarMetrics, ThreadRowView, ProjectTiles).

/// `Color.primary` at an opacity: white in dark mode, black in light.
pub fn primary_opacity(cx: &App, alpha: f32) -> Hsla {
    pick(cx, white(alpha), black(alpha))
}

/// A row under the pointer (SidebarMetrics.hover).
pub fn sidebar_hover(cx: &App) -> Hsla {
    primary_opacity(cx, 0.03)
}

/// The open thread's row, brighter than a hover so the two read apart.
pub fn sidebar_selected(cx: &App) -> Hsla {
    primary_opacity(cx, 0.08)
}

/// Theme.failed.
pub fn failed(cx: &App) -> Hsla {
    pick(cx, rgb(0xff453a).into(), hsla(0.0, 0.71, 0.46, 1.0))
}

/// `.secondary` text (secondaryLabelColor).
pub fn secondary_text(cx: &App) -> Hsla {
    pick(cx, white(0.55), black(0.5))
}

/// `SidebarMetrics.corner`: the rails' lit square.
pub const RAIL_CORNER: f32 = 8.;

/// The connection banner's warning fill (`.yellow.opacity(0.12)`).
pub fn warning_fill(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.8, 0., 0.12), srgb(1., 0.8, 0., 0.12))
}

/// The tick on the end-of-inbox note (`Color.green`).
pub fn tick_green(cx: &App) -> Hsla {
    pick(cx, srgb(0.2, 0.83, 0.4, 1.), srgb(0.2, 0.7, 0.3, 1.))
}

// A playground for BearDock on GPUI: mock panels to drag about until docking
// feels right, as dock/Sources/DockDemo is for the Swift one. The layout is saved
// between launches; Reset puts it back. Run: `cargo run -p threadbear-dock --bin dock-demo`.
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;
use threadbear_dock::{DockContent, DockDrop, DockEdge, DockGroup, DockLayout, DockModel, DockNode, DockOptions, DockSplit, DockView, SplitAxis};

fn group(id: &str, panels: &[&str]) -> DockNode {
    DockNode::Group(DockGroup::new(id, panels.iter().map(|p| p.to_string()).collect(), None))
}

/// Option 2's three columns, with Threads' title hidden.
fn starting() -> DockLayout {
    DockLayout::new(
        DockNode::Split(DockSplit::new(
            "root",
            SplitAxis::Horizontal,
            vec![
                group("left", &["threads"]),
                group("middle", &["transcript"]),
                DockNode::Split(DockSplit::new(
                    "right",
                    SplitAxis::Vertical,
                    vec![group("right-top", &["highlights", "worktrees"]), group("right-bottom", &["convex"])],
                    Some(vec![0.55, 0.45]),
                )),
            ],
            Some(vec![0.24, 0.5, 0.26]),
        )),
        ["threads".to_string()],
        Vec::new(),
    )
}

fn layout_file() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join(".beardock-demo-layout.json")
}

fn saved() -> Option<DockLayout> {
    if std::env::var_os("BEARDOCK_FRESH").is_some() {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(layout_file()).ok()?).ok()
}

fn save(layout: &DockLayout) {
    // A fresh run (screenshots, tests) leaves the saved layout alone.
    if std::env::var_os("BEARDOCK_FRESH").is_some() {
        return;
    }
    if let Ok(text) = serde_json::to_string(layout) {
        let _ = std::fs::write(layout_file(), text);
    }
}

struct Demo;

impl DockContent for Demo {
    fn title(&self, panel: &str, _: &App) -> SharedString {
        match panel {
            "threads" => "Threads",
            "transcript" => "Transcript",
            "highlights" => "Highlights",
            "convex" => "Convex dashboard",
            "worktrees" => "Worktrees",
            "terminal" => "Terminal",
            "files" => "Files",
            other => return other.to_string().into(),
        }
        .into()
    }

    fn icon(&self, panel: &str, _: &mut Window, _: &mut App) -> Option<AnyElement> {
        use gpui_kit::assets::IconName as L;
        let icon = match panel {
            "threads" => L::Inbox,
            "transcript" => L::FileText,
            "highlights" => L::Star,
            "convex" => L::LayoutDashboard,
            "worktrees" => L::Network,
            "terminal" => L::SquareTerminal,
            "files" => L::Folder,
            _ => return None,
        };
        Some(svg().path(icon.path()).size(rems(0.72)).text_color(rgb(0x8e8e93)).into_any_element())
    }

    fn view(&self, panel: &str, _ghost: bool, _: &mut Window, cx: &mut App) -> AnyElement {
        mock_panel(panel, cx)
    }

    fn closable(&self, panel: &str, _: &App) -> bool {
        panel != "transcript"
    }
}

/// Stand-ins for Threadbear's panels: enough texture to see what's where.
/// Sized in rems, so the ghost can scale them.
fn mock_panel(panel: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let faint = theme.muted_foreground;
    let text = theme.foreground;
    let tint = text.opacity(0.08);
    let rows = |lines: &[(&str, &str, &str)]| {
        v_flex().gap(rems(0.15)).p(rems(0.5)).children(lines.iter().map(|(a, b, c)| {
            h_flex()
                .gap(rems(0.6))
                .p(rems(0.5))
                .rounded(rems(0.5))
                .child(div().size(rems(1.75)).rounded(rems(0.4)).bg(tint))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(rems(0.))
                        .child(div().text_size(rems(0.8)).font_weight(FontWeight::MEDIUM).truncate().child(a.to_string()))
                        .child(div().text_size(rems(0.69)).text_color(faint).child(b.to_string())),
                )
                .child(div().text_size(rems(0.69)).text_color(faint).child(c.to_string()))
        }))
    };
    match panel {
        "threads" => rows(&[
            ("Docking library and demo app", "threadbear", "now"),
            ("Mobile app for Threadbear", "threadbear", "1m"),
            ("Tidy the pool settings screen", "misc", "8m"),
            ("Resume Convex state machine work", "convex-native-v7", "14h"),
            ("Next video after Nippon", "tandem", "15h"),
            ("Projects page grid view", "misc", "15h"),
            ("Poddle: download one episode", "poddle", "1d"),
        ])
        .into_any_element(),
        "transcript" => v_flex()
            .size_full()
            .p(rems(1.25))
            .gap(rems(0.75))
            .text_size(rems(0.85))
            .child("Model tests pass. Now the views: a controller that owns the layout and the drag, a recursive view for the tree, and an overlay for the ghost, the preview and the compass.")
            .child("Dragging a panel lifts it into a small floating card under the pointer. Over another panel, the side you're nearest lights up; in the middle, a compass lets you choose a side, a tab or a swap.")
            .child(div().flex().justify_end().child(div().p(rems(0.75)).rounded(rems(0.75)).bg(tint).child("Can you make it shrink a bit as you pick it up?")))
            .child("Done. It scales to about 240 points wide with the point you grabbed kept under the pointer.")
            .child(div().flex_1())
            .child(div().h(rems(5.25)).rounded(rems(1.)).border_1().border_color(tint).p(rems(0.9)).text_color(faint).child("Message"))
            .into_any_element(),
        "highlights" => v_flex()
            .p(rems(0.9))
            .gap(rems(0.6))
            .text_size(rems(0.8))
            .text_color(faint)
            .child("Building a docking library with a demo app, so dragging panels can be tuned before it goes into Threadbear.")
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Changes"))
            .children(["DockLayout: tree, moves, tidy-up", "Ghost, preview and compass", "Demo app with mock panels"].map(|line| {
                h_flex().gap(rems(0.5)).child(div().text_size(rems(0.7)).child("a1b2c3d")).child(div().text_color(text).child(line))
            }))
            .into_any_element(),
        "convex" => v_flex()
            .child(h_flex().p(rems(0.75)).gap(rems(0.4)).child(div().text_size(rems(0.75)).font_weight(FontWeight::SEMIBOLD).child("threads")).child(div().text_size(rems(0.69)).text_color(faint).child("214 documents")))
            .children((0..9).map(|i| {
                h_flex()
                    .px(rems(0.75))
                    .py(rems(0.4))
                    .gap(rems(0.75))
                    .when(i % 2 == 0, |d| d.bg(text.opacity(0.025)))
                    .child(div().text_size(rems(0.69)).text_color(faint).child(format!("k17{i}f…")))
                    .child(div().text_size(rems(0.72)).child(["running", "idle", "needsYou", "idle", "archived"][i % 5]))
                    .child(div().flex_1())
                    .child(div().text_size(rems(0.69)).text_color(faint).child(format!("{}m", i + 2)))
            }))
            .into_any_element(),
        "worktrees" => v_flex()
            .p(rems(0.9))
            .gap(rems(0.3))
            .text_size(rems(0.72))
            .child(div().text_color(faint).child("Rows you could reorder; the panel's own drag would win over the dock's."))
            .children(["thread/419a9c2a  dock", "thread/89b52654  convex backend", "thread/2f1e77c0  pool settings", "thread/7c3d9e11  website"].map(|w| {
                div().h(rems(1.9)).px(rems(0.5)).flex().items_center().rounded(rems(0.4)).bg(text.opacity(0.03)).child(w)
            }))
            .into_any_element(),
        "terminal" => v_flex()
            .p(rems(0.75))
            .gap(rems(0.2))
            .text_size(rems(0.72))
            .child("$ cargo test -p threadbear-dock-layout")
            .child(div().text_color(rgb(0x72d982)).child("test result: ok. 32 passed; 0 failed"))
            .child("$ cargo run -p threadbear-dock --bin dock-demo")
            .child(div().text_color(faint).child("Building for debugging..."))
            .into_any_element(),
        "files" => v_flex()
            .p(rems(0.9))
            .gap(rems(0.4))
            .text_size(rems(0.75))
            .children(["dock/", "  Package.swift", "  Sources/BearDock/", "    DockLayout.swift", "    DockController.swift", "    DockView.swift", "    DragOverlay.swift"].map(|f| div().child(f)))
            .into_any_element(),
        other => div().p(rems(0.75)).text_color(faint).child(other.to_string()).into_any_element(),
    }
}

#[cfg(feature = "automation")]
mod script;

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx: &mut App| {
        gpui_kit::init(cx);
        let mode = match std::env::var("BEARDOCK_APPEARANCE").as_deref() {
            Ok("light") => ThemeMode::Light,
            _ => ThemeMode::Dark,
        };
        Theme::change(mode, None, cx);

        let bounds = Bounds::centered(None, size(px(1320.), px(820.)), cx);
        let opened = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("BearDock".into()), appears_transparent: true, traffic_light_position: None }),
                // The dock's bars are the window's top edge. Without this AppKit
                // treats them as a title bar and moves the window on a drag there,
                // as BearDock's DockAnchor exists to stop. Any app embedding the
                // dock in a window with no title bar needs the same.
                app_owns_titlebar_drag: true,
                window_min_size: Some(size(px(800.), px(500.))),
                app_id: Some("beardock-demo".into()),
                ..Default::default()
            },
            cx,
            |_, cx| {
                let layout = saved().unwrap_or_else(starting);
                let model = cx.new(|_| {
                    let mut m = DockModel::new(layout);
                    m.on_change(|layout, _| save(layout));
                    m
                });
                // The Mac's system blue, so the demo's targets look like Swift's.
                let accent = rgb(0x0a84ff).into();
                let options = DockOptions {
                    traffic_lights: 78.,
                    corner_moves_window: true,
                    accent: Some(accent),
                    // The app's bars move its window; this tries them the same way.
                    bars_move_window: std::env::var_os("BEARDOCK_BARS_MOVE_WINDOW").is_some(),
                    ..DockOptions::default()
                };
                cx.new(|cx| DemoRoot { dock: cx.new(|cx| DockView::new(model.clone(), Rc::new(Demo), options, cx)), model })
            },
        )
        .expect("failed to open the window");
        #[cfg(feature = "automation")]
        script::run(opened.0, cx);
        #[cfg(not(feature = "automation"))]
        let _ = opened;
    });
}

/// The demo's window: the dock under a small toolbar to reopen closed panels.
struct DemoRoot {
    dock: Entity<DockView>,
    model: Entity<DockModel>,
}

impl Render for DemoRoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.clone();
        let layout = model.read(cx).layout().clone();
        let closed: Vec<&str> = ["threads", "transcript", "highlights", "convex", "worktrees", "terminal", "files"].into_iter().filter(|p| !layout.contains(p)).collect();
        // The dock fills the window, so the traffic lights sit in the top-left
        // panel's bar (DockOptions::traffic_lights); the demo's own buttons
        // live in a strip along the bottom.
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(div().flex_1().min_h(px(0.)).child(self.dock.clone()))
            .child(
                h_flex()
                    .flex_none()
                    .h(px(26.))
                    .px(px(10.))
                    .gap(px(10.))
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .children(closed.into_iter().map(|p| {
                        let m = model.clone();
                        div().id(p).cursor_pointer().child(format!("Open {p}")).on_click(move |_, _, cx| {
                            m.update(cx, |m, cx| m.open(p, DockDrop::window(DockEdge::Right), cx));
                        })
                    }))
                    .child(div().flex_1())
                    .child(div().id("reset").cursor_pointer().child("Reset layout").on_click({
                        let m = model.clone();
                        move |_, _, cx| m.update(cx, |m, cx| m.set_layout(starting(), cx))
                    })),
            )
    }
}

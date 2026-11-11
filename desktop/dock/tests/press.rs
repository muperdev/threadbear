// The press rules through the real views: a headless window with real layout
// and hit testing (gpui_kit::test), which the pure layout crate's tests can't
// reach. Run with `cargo test -p threadbear-dock --features automation --test press`
// (on a machine that isn't Mike's, like every GPUI build; desktop/scripts/remote.sh).
use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
// Named, not `gpui_kit::*`: a glob brings `gpui_kit::test` into scope, which
// shadows the built-in #[test] that #[gpui_kit::test] expands to.
use gpui_kit::{div, px, size, AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Entity, IntoElement as _, MouseButton, ParentElement as _, Pixels, SharedString, Styled as _, TestAppContext, Window, WindowBounds, WindowOptions};
use std::rc::Rc;
use std::time::Duration;
use threadbear_dock::*;

struct Panels;

impl DockContent for Panels {
    fn title(&self, panel: &str, _: &App) -> SharedString {
        panel.to_string().into()
    }

    fn view(&self, panel: &str, _ghost: bool, _: &mut Window, _: &mut App) -> AnyElement {
        div().size_full().child(panel.to_string()).into_any_element()
    }
}

fn group(id: &str, panels: &[&str]) -> DockNode {
    DockNode::Group(DockGroup::new(id, panels.iter().map(|p| p.to_string()).collect(), None))
}

fn two_groups() -> DockLayout {
    DockLayout::new(DockNode::Split(DockSplit::new("root", SplitAxis::Horizontal, vec![group("left", &["a"]), group("right", &["b", "c"])], None)), Vec::<String>::new(), Vec::new())
}

/// A headless window with the dock in it, and its model.
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DockModel>) {
    cx.update(gpui_kit::init);
    let model = cx.new(|_| DockModel::new(two_groups()));
    let m = model.clone();
    let handle = cx.update(|cx| {
        let bounds = Bounds { origin: gpui_kit::Point::default(), size: size(px(1000.), px(600.)) };
        gpui_kit::open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, cx, move |_, cx| cx.new(|cx| DockView::new(m, Rc::new(Panels), DockOptions::default(), cx))).expect("a test window").0
    });
    // Draw once so the bars and frames are measured.
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
    (handle, model)
}

fn frame(cx: &mut TestAppContext, model: &Entity<DockModel>, id: &str) -> Rect {
    cx.read(|cx| *model.read(cx).controller.frames.get(id).expect("the group was drawn"))
}

fn bar(cx: &mut TestAppContext, model: &Entity<DockModel>, id: &str) -> Rect {
    cx.read(|cx| *model.read(cx).controller.bars.get(id).expect("the bar was drawn"))
}

fn at(r: Rect, x: f64, y: f64) -> gpui_kit::Point<Pixels> {
    gpui_kit::point(px((r.x + x) as f32), px((r.y + y) as f32))
}

fn drag_panel(cx: &mut TestAppContext, model: &Entity<DockModel>) -> Option<String> {
    cx.read(|cx| model.read(cx).controller.drag.as_ref().map(|d| d.panel.clone()))
}

fn summary(cx: &mut TestAppContext, model: &Entity<DockModel>) -> String {
    cx.read(|cx| model.read(cx).layout().summary())
}

#[gpui_kit::test]
async fn a_title_picks_the_panel_up_five_points_in_with_no_hold(cx: &mut TestAppContext) {
    let (handle, model) = open(cx);
    let left_bar = bar(cx, &model, "left");
    // The title is in the bar, the panel's group is just past it.
    let grip = at(left_bar, 13., 15.);
    cx.update_window(handle, |_, w, cx| {
        w.pointer_move(grip, None, cx);
        w.pointer_down(grip, MouseButton::Left, cx);
        // Less than five points: nothing yet.
        w.pointer_move(grip + gpui_kit::point(px(3.), px(0.)), Some(MouseButton::Left), cx);
    })
    .unwrap();
    assert_eq!(drag_panel(cx, &model), None);
    cx.update_window(handle, |_, w, cx| w.pointer_move(grip + gpui_kit::point(px(40.), px(30.)), Some(MouseButton::Left), cx)).unwrap();
    assert_eq!(drag_panel(cx, &model), Some("a".to_string()));
    // Dropped on another group's bar it joins as a tab; the bar's own
    // space is further along than the tabs.
    let right = bar(cx, &model, "right");
    let there = at(right, 200., 15.);
    cx.update_window(handle, |_, w, cx| {
        w.pointer_move(there, Some(MouseButton::Left), cx);
        w.pointer_up(there, MouseButton::Left, cx);
    })
    .unwrap();
    assert_eq!(drag_panel(cx, &model), None);
    cx.run_until_parked();
    assert!(summary(cx, &model).contains("a"), "the layout is {}", summary(cx, &model));
    assert_eq!(cx.read(|cx| model.read(cx).layout().group("right").map(|g| g.panels.len())), Some(3));
}

#[gpui_kit::test]
async fn a_body_press_does_nothing_without_a_hold(cx: &mut TestAppContext) {
    let (handle, model) = open(cx);
    let body = frame(cx, &model, "right");
    let p = at(body, 200., 300.);
    cx.update_window(handle, |_, w, cx| {
        w.pointer_move(p, None, cx);
        w.pointer_down(p, MouseButton::Left, cx);
        w.pointer_move(p + gpui_kit::point(px(20.), px(0.)), Some(MouseButton::Left), cx);
    })
    .unwrap();
    // Moved before the hold was up: not a panel drag, and moved on a
    // background it does nothing (Mike, 28 Oct).
    assert_eq!(drag_panel(cx, &model), None);
    cx.update_window(handle, |_, w, cx| w.pointer_up(p, MouseButton::Left, cx)).unwrap();

    // Held first: the panel comes up when it moves.
    cx.update_window(handle, |_, w, cx| {
        w.pointer_move(p, None, cx);
        w.pointer_down(p, MouseButton::Left, cx);
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    cx.update_window(handle, |_, w, cx| w.pointer_move(p + gpui_kit::point(px(40.), px(0.)), Some(MouseButton::Left), cx)).unwrap();
    assert_eq!(drag_panel(cx, &model), Some("b".to_string()));
    cx.update_window(handle, |_, w, cx| w.pointer_up(p, MouseButton::Left, cx)).unwrap();
}

#[gpui_kit::test]
async fn escape_cancels_the_drag_without_moving_anything(cx: &mut TestAppContext) {
    let (handle, model) = open(cx);
    let before = summary(cx, &model);
    let tab = at(bar(cx, &model, "right"), 13., 15.);
    let elsewhere = at(frame(cx, &model, "left"), 150., 300.);
    cx.update_window(handle, |_, w, cx| {
        w.pointer_move(tab, None, cx);
        w.pointer_down(tab, MouseButton::Left, cx);
        w.pointer_move(elsewhere, Some(MouseButton::Left), cx);
    })
    .unwrap();
    assert!(drag_panel(cx, &model).is_some());
    cx.update_window(handle, |_, w, cx| w.key_down("escape", false, cx)).unwrap();
    assert_eq!(drag_panel(cx, &model), None);
    cx.run_until_parked();
    assert_eq!(summary(cx, &model), before);
    cx.update_window(handle, |_, w, cx| w.pointer_up(elsewhere, MouseButton::Left, cx)).unwrap();
    assert_eq!(summary(cx, &model), before);
}

#[gpui_kit::test]
async fn a_divider_drag_moves_the_share_and_stays_above_the_minimum(cx: &mut TestAppContext) {
    let (handle, model) = open(cx);
    let left = frame(cx, &model, "left");
    let on_divider = gpui_kit::point(px(left.max_x() as f32), px(300.));
    cx.update_window(handle, |_, w, cx| {
        w.pointer_move(on_divider, None, cx);
        w.pointer_down(on_divider, MouseButton::Left, cx);
        w.pointer_move(gpui_kit::point(px(left.max_x() as f32 + 100.), px(300.)), Some(MouseButton::Left), cx);
        w.pointer_up(gpui_kit::point(px(left.max_x() as f32 + 100.), px(300.)), MouseButton::Left, cx);
    })
    .unwrap();
    let DockNode::Split(root) = cx.read(|cx| model.read(cx).layout().root.clone()) else { panic!("expected a split") };
    assert!((root.fractions[0] - 0.6).abs() < 0.01, "the left share is {}", root.fractions[0]);
    // Dragged all the way to the edge it stops at the 80-point minimum.
    cx.update_window(handle, |_, w, cx| {
        let p = gpui_kit::point(px(left.max_x() as f32 + 100.), px(300.));
        w.pointer_move(p, None, cx);
        w.pointer_down(p, MouseButton::Left, cx);
        w.pointer_move(gpui_kit::point(px(5.), px(300.)), Some(MouseButton::Left), cx);
        w.pointer_up(gpui_kit::point(px(5.), px(300.)), MouseButton::Left, cx);
    })
    .unwrap();
    let DockNode::Split(root) = cx.read(|cx| model.read(cx).layout().root.clone()) else { panic!("expected a split") };
    assert!(root.fractions[0] >= 80. / 999. && root.fractions[0] < 0.1, "the left share is {}", root.fractions[0]);
}

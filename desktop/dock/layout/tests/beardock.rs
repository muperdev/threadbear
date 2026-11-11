// BearDock's 31 tests, ported from dock/Tests/BearDockTests/ (DockLayoutTests,
// BarDropTests, PanelAtPointTests) under the same names, so a test here and its
// Swift original can be compared by name. Rust would prefer snake_case; the
// exact names are worth more here than the style warning.
#![allow(non_snake_case)]

use std::collections::HashMap;
use threadbear_dock_layout::*;

// Small builders so each tree reads like the picture of it.
fn g(id: &str, panels: &[&str]) -> DockNode {
    DockNode::Group(DockGroup::new(id, panels.iter().map(|p| p.to_string()).collect(), None))
}
fn gs(id: &str, panels: &[&str], selected: &str) -> DockNode {
    DockNode::Group(DockGroup::new(id, panels.iter().map(|p| p.to_string()).collect(), Some(selected.to_string())))
}
fn h(id: &str, children: Vec<DockNode>, fractions: &[f64]) -> DockNode {
    let f = if fractions.is_empty() { None } else { Some(fractions.to_vec()) };
    DockNode::Split(DockSplit::new(id, SplitAxis::Horizontal, children, f))
}
fn v(id: &str, children: Vec<DockNode>, fractions: &[f64]) -> DockNode {
    let f = if fractions.is_empty() { None } else { Some(fractions.to_vec()) };
    DockNode::Split(DockSplit::new(id, SplitAxis::Vertical, children, f))
}
/// "n1", "n2", ... so new ids are predictable.
fn ids() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("n{n}")
    }
}
fn layout(root: DockNode) -> DockLayout {
    DockLayout::new(root, Vec::<String>::new(), Vec::new())
}

/// Threadbear's option 2: threads | transcript | highlights over convex.
fn threeColumns() -> DockLayout {
    layout(h(
        "root",
        vec![g("left", &["threads"]), g("mid", &["transcript"]), v("right", vec![g("hl", &["highlights"]), g("cx", &["convex"])], &[])],
        &[0.25, 0.5, 0.25],
    ))
}

#[test]
fn edgeAlongTheSameAxisJoinsAsASiblingSharingTheTargetsSpace() {
    let mut layout = threeColumns();
    layout.move_panel_with("convex", DockDrop::edge("mid", DockEdge::Right), &mut ids());
    assert_eq!(
        layout.root,
        h("root", vec![g("left", &["threads"]), g("mid", &["transcript"]), g("n1", &["convex"]), g("hl", &["highlights"])], &[0.25, 0.25, 0.25, 0.25])
    );
}

#[test]
fn edgeAcrossTheAxisWrapsTheTargetInANewSplit() {
    let mut layout = threeColumns();
    layout.move_panel_with("threads", DockDrop::edge("mid", DockEdge::Bottom), &mut ids());
    // threads' column went, so its quarter is shared between the rest.
    assert_eq!(
        layout.root,
        h(
            "root",
            vec![v("n2", vec![g("mid", &["transcript"]), g("n1", &["threads"])], &[]), v("right", vec![g("hl", &["highlights"]), g("cx", &["convex"])], &[])],
            &[0.5 / 0.75, 0.25 / 0.75]
        )
    );
}

#[test]
fn nestingGoesAsDeepAsYouLike() {
    let mut layout = threeColumns();
    let mut next = ids();
    layout.move_panel_with("threads", DockDrop::edge("cx", DockEdge::Left), &mut next); // n1, in split n2
    layout.move_panel_with("highlights", DockDrop::edge("n1", DockEdge::Top), &mut next); // n3, in split n4
    layout.move_panel_with("convex", DockDrop::edge("n1", DockEdge::Right), &mut next); // n5, in split n6
    // Three levels: a row, a column in it, a row in that.
    let DockNode::Split(root) = &layout.root else { panic!("expected a split") };
    assert_eq!(root.children, vec![g("mid", &["transcript"]), v("n4", vec![g("n3", &["highlights"]), h("n6", vec![g("n1", &["threads"]), g("n5", &["convex"])], &[])], &[])]);
    // threads' quarter went to the right, then convex's sixth back to both.
    assert!((root.fractions[0] - 0.8).abs() < 1e-9);
}

#[test]
fn movingTheOnlyPanelOutCollapsesItsGroupAndSplit() {
    let mut layout = threeColumns();
    layout.move_panel("highlights", DockDrop::tab("mid"));
    // The right column is down to convex alone, so it's just that group now.
    assert_eq!(layout.root, h("root", vec![g("left", &["threads"]), gs("mid", &["transcript", "highlights"], "highlights"), g("cx", &["convex"])], &[0.25, 0.5, 0.25]));
}

#[test]
fn sameAxisSplitsFlatten() {
    let node = h("a", vec![g("x", &["X"]), h("b", vec![g("y", &["Y"]), g("z", &["Z"])], &[])], &[0.5, 0.5]);
    assert_eq!(DockLayout::normalised(&node), h("a", vec![g("x", &["X"]), g("y", &["Y"]), g("z", &["Z"])], &[0.5, 0.25, 0.25]));
}

#[test]
fn takingASelectedTabShowsItsNeighbour() {
    let mut layout = self::layout(h("root", vec![gs("a", &["A", "B", "C"], "B"), g("d", &["D"])], &[]));
    layout.move_panel("B", DockDrop::tab("d"));
    assert_eq!(layout.group("a").map(|g| g.selected), Some("C".to_string()));
    assert_eq!(layout.group("d"), Some(DockGroup::new("d", vec!["D".into(), "B".into()], Some("B".into()))));
    layout.move_panel("C", DockDrop::tab("d"));
    assert_eq!(layout.group("a").map(|g| g.selected), Some("A".to_string()));
}

#[test]
fn splittingAGroupByOneOfItsOwnTabs() {
    let mut layout = self::layout(g("a", &["A", "B"]));
    layout.move_panel_with("B", DockDrop::edge("a", DockEdge::Right), &mut ids());
    assert_eq!(layout.root, h("n2", vec![g("a", &["A"]), g("n1", &["B"])], &[]));
}

#[test]
fn swapTradesPlaces() {
    let mut layout = threeColumns();
    layout.move_panel("threads", DockDrop::swap("cx"));
    assert_eq!(layout.group("left").map(|g| g.panels), Some(vec!["convex".to_string()]));
    assert_eq!(layout.group("cx"), Some(DockGroup::new("cx", vec!["threads".into()], None)));
}

#[test]
fn windowEdgesSpanTheWholeSide() {
    let mut layout = threeColumns();
    let mut next = ids();
    layout.move_panel_with("convex", DockDrop::window(DockEdge::Right), &mut next);
    assert_eq!(
        layout.root,
        h("root", vec![g("left", &["threads"]), g("mid", &["transcript"]), g("hl", &["highlights"]), g("n1", &["convex"])], &[0.1875, 0.375, 0.1875, 0.25])
    );

    layout.move_panel_with("threads", DockDrop::window(DockEdge::Bottom), &mut next);
    let DockNode::Split(root) = &layout.root else { panic!("expected a split") };
    assert_eq!(root.axis, SplitAxis::Vertical);
    assert_eq!(root.fractions, vec![0.75, 0.25]);
    assert_eq!(root.children.last(), Some(&g("n2", &["threads"])));
}

#[test]
fn dropsThatGoNowhereAreRefused() {
    let layout = threeColumns();
    assert!(!layout.can_drop("threads", &DockDrop::edge("left", DockEdge::Right)));
    assert!(!layout.can_drop("threads", &DockDrop::tab("left")));
    assert!(!layout.can_drop("threads", &DockDrop::swap("left")));
    assert!(!layout.can_drop("nope", &DockDrop::tab("left")));
    assert!(!layout.can_drop("threads", &DockDrop::tab("nope")));
    assert!(!self::layout(g("a", &["A"])).can_drop("A", &DockDrop::window(DockEdge::Left)));
    assert!(layout.can_drop("threads", &DockDrop::edge("mid", DockEdge::Left)));
    let mut unchanged = layout.clone();
    unchanged.move_panel("threads", DockDrop::tab("left"));
    assert_eq!(unchanged, layout);
}

#[test]
fn closeAndAddBack() {
    let mut layout = threeColumns();
    layout.close("convex");
    assert!(!layout.contains("convex"));
    layout.add("convex", DockDrop::tab("hl"));
    assert_eq!(layout.group("hl").map(|g| g.panels), Some(vec!["highlights".to_string(), "convex".to_string()]));
    let mut single = self::layout(g("a", &["A"]));
    single.close("A");
    assert_eq!(single.root, g("a", &["A"]));
}

#[test]
fn dividerFractionsStayNormalised() {
    let mut layout = threeColumns();
    layout.set_fractions("root", &[1., 2., 1.]);
    let DockNode::Split(root) = &layout.root else { panic!("expected a split") };
    assert_eq!(root.fractions, vec![0.25, 0.5, 0.25]);
}

#[test]
fn roundTripsThroughJSON() {
    let layout = threeColumns();
    let data = serde_json::to_string(&layout).expect("encodes");
    assert_eq!(serde_json::from_str::<DockLayout>(&data).expect("decodes"), layout);
}

#[test]
fn aHiddenTitleOnlyHidesWhileThePanelIsOnItsOwn() {
    let mut layout = threeColumns();
    layout.set_title_hidden("threads", true);
    assert!(!layout.shows_header(&layout.group("left").unwrap()));
    // Joined by a tab: the tabs are needed to switch, so the bar comes back.
    layout.move_panel("transcript", DockDrop::tab("left"));
    assert!(layout.shows_header(&layout.group_containing("threads").unwrap()));
    // On its own again, wherever it ends up, the title stays hidden.
    layout.move_panel("threads", DockDrop::window(DockEdge::Right));
    assert!(!layout.shows_header(&layout.group_containing("threads").unwrap()));
    layout.set_title_hidden("threads", false);
    assert!(layout.shows_header(&layout.group_containing("threads").unwrap()));
}

#[test]
fn layoutsSavedBeforeHiddenTitlesStillLoad() {
    let old = r#"{"root":{"group":{"_0":{"id":"a","panels":["A"],"selected":"A"}}}}"#;
    let layout: DockLayout = serde_json::from_str(old).expect("decodes");
    assert!(layout.hidden_titles.is_empty());
    assert_eq!(layout.panels(), vec!["A".to_string()]);
}

#[test]
fn theTopLeadingGroupIsTheFirstChildAllTheWayDown() {
    assert_eq!(threeColumns().top_leading_group_id(), "left");
    let mut layout = threeColumns();
    layout.move_panel_with("highlights", DockDrop::window(DockEdge::Top), &mut ids());
    assert_eq!(layout.top_leading_group_id(), "n1");
}

#[test]
fn showAndToggleFromARail() {
    let right = DockDrop::window(DockEdge::Right);
    let mut dock = DockController::new(threeColumns());
    // Open behind another tab: brought to the front.
    dock.layout.move_panel("convex", DockDrop::tab("hl"));
    assert!(!dock.is_showing("highlights"));
    dock.toggle("highlights", &[], &right);
    assert!(dock.is_showing("highlights"));
    // Showing: closed.
    dock.toggle("highlights", &[], &right);
    assert!(!dock.layout.contains("highlights"));
    // Closed: opened as a tab beside the first of `near` that's open.
    dock.toggle("highlights", &["nope", "convex"], &right);
    assert_eq!(dock.layout.group_containing("highlights").map(|g| g.panels), Some(vec!["convex".to_string(), "highlights".to_string()]));
}

#[test]
fn summaryReadsLikeTheWindow() {
    let mut layout = threeColumns();
    layout.set_title_hidden("threads", true);
    layout.move_panel("convex", DockDrop::tab("hl"));
    assert_eq!(layout.summary(), "row[threads (no title) 25% | transcript 50% | highlights+convex* 25%]");
}

#[test]
fn unavailablePanelsAreLeftOutOfTheirGroup() {
    let group = DockGroup::new("g", vec!["highlights".into(), "convex".into(), "simulator".into()], Some("simulator".into()));
    let drawn = visible_group(&group, &|p: &str| p != "simulator");
    assert_eq!(drawn, DockGroup::new("g", vec!["highlights".into(), "convex".into()], Some("highlights".into())));
    // Nothing available: drawn as it is, for the app's own note.
    assert_eq!(visible_group(&group, &|_: &str| false), group);
}

#[test]
fn aSideWithNothingToShowGivesItsRoomToTheRest() {
    // A thread's patch panel moved to a side of its own, while a thread
    // without it is on show: that side goes and the others share its room.
    let split = DockSplit::new(
        "root",
        SplitAxis::Horizontal,
        vec![g("tool", &["tandem"]), g("mid", &["transcript"]), g("right", &["highlights", "convex"])],
        Some(vec![0.25, 0.5, 0.25]),
    );
    let shown = shown_indices(&split, &|p: &str| p != "tandem" && p != "convex");
    assert_eq!(shown, vec![1, 2]);
    let shares: Vec<f64> = normalized(&shown.iter().map(|&i| split.fractions[i]).collect::<Vec<_>>());
    let expected = vec![2.0 / 3.0, 1.0 / 3.0];
    assert_eq!(shares, expected);
    // A divider dragged meanwhile keeps the hidden side's share.
    assert_eq!(merged(&[0.5, 0.5], &split.fractions, &shown), vec![0.25, 0.375, 0.375]);
    // Nothing available anywhere: all of it, as a group does.
    assert_eq!(shown_indices(&split, &|_: &str| false), vec![0, 1, 2]);
    // The traffic lights move on to the next side.
    let node = DockNode::Split(split);
    assert_eq!(top_leading_group_id(&node, &|p: &str| p != "tandem"), "mid");
    assert_eq!(top_leading_group_id(&node, &|_: &str| true), "tool");
}

// MARK: Windows of their own

#[test]
fn floatingTakesAPanelOutOfTheTreeButKeepsItOpen() {
    let mut layout = threeColumns();
    let frame = Rect::new(100., 200., 400., 300.);
    layout.float("highlights", frame, None, None);
    assert_eq!(layout.root, h("root", vec![g("left", &["threads"]), g("mid", &["transcript"]), g("cx", &["convex"])], &[0.25, 0.5, 0.25]));
    assert_eq!(layout.floating, vec![DockFloat::new("highlights", frame, None, None)]);
    assert!(layout.contains("highlights"));
    assert_eq!(layout.group_containing("highlights"), None);
    assert_eq!(layout.panels().last().map(|s| s.as_str()), Some("highlights"));
    assert!(layout.summary().ends_with(" + window[highlights]"));
}

#[test]
fn floatingAgainOnlyMovesTheWindow() {
    let mut layout = threeColumns();
    layout.float("convex", Rect::new(0., 0., 300., 300.), None, None);
    layout.float("convex", Rect::new(50., 60., 320., 340.), None, None);
    assert_eq!(layout.floating, vec![DockFloat::new("convex", Rect::new(50., 60., 320., 340.), None, None)]);
}

#[test]
fn theLastPanelInTheDockCantFloat() {
    let mut layout = self::layout(g("only", &["transcript"]));
    assert!(!layout.can_float("transcript"));
    layout.float("transcript", Rect::ZERO, None, None);
    assert!(layout.floating.is_empty());
    assert_eq!(layout.root, g("only", &["transcript"]));
}

#[test]
fn aFloatingPanelDocksBackAnywhereButASwap() {
    let mut layout = threeColumns();
    layout.float("threads", Rect::ZERO, None, None);
    assert!(layout.can_drop("threads", &DockDrop::tab("mid")));
    assert!(layout.can_drop("threads", &DockDrop::edge("mid", DockEdge::Left)));
    assert!(layout.can_drop("threads", &DockDrop::window(DockEdge::Left)));
    assert!(!layout.can_drop("threads", &DockDrop::swap("mid")));
    layout.move_panel_with("threads", DockDrop::edge("hl", DockEdge::Top), &mut ids());
    assert!(layout.floating.is_empty());
    assert_eq!(layout.group_containing("threads").map(|g| g.id), Some("n1".to_string()));
}

#[test]
fn closingAFloatingPanelClosesItsWindow() {
    let mut layout = threeColumns();
    layout.float("convex", Rect::ZERO, None, None);
    layout.close("convex");
    assert!(layout.floating.is_empty());
    assert!(!layout.contains("convex"));
}

#[test]
fn layoutsSavedBeforeWindowsStillLoad() {
    let mut old: serde_json::Value = serde_json::to_value(threeColumns()).expect("encodes");
    old.as_object_mut().expect("an object").remove("floating");
    let decoded: DockLayout = serde_json::from_value(old).expect("decodes");
    assert!(decoded.floating.is_empty());
    let mut floated = threeColumns();
    floated.float("convex", Rect::new(1., 2., 3., 4.), None, None);
    let round: DockLayout = serde_json::from_str(&serde_json::to_string(&floated).expect("encodes")).expect("decodes");
    assert_eq!(round, floated);
}

#[test]
fn putBackAlongASideComesBackTheSizeItWas() {
    let mut layout = self::layout(h("root", vec![g("left", &["threads"]), g("mid", &["transcript"])], &[0.3, 0.7]));
    layout.float("transcript", Rect::ZERO, Some(DockDrop::window(DockEdge::Right)), Some(0.7));
    layout.move_panel_with("transcript", DockDrop::window(DockEdge::Right), &mut ids());
    let DockNode::Split(s) = &layout.root else { panic!("expected a split") };
    assert_eq!(s.children, vec![g("left", &["threads"]), g("n1", &["transcript"])]);
    assert!((s.fractions[1] - 0.7).abs() < 0.0001);
}

// Beyond Swift's 31: the JSON shape is Swift's Codable, so that a layout saved
// by the Mac app (UserDefaults `dockLayout`) loads here. Written by hand from
// what Codable produces: `{"case":{"_0":value}}` for unlabelled associated
// values, labelled ones by name, `[[x,y],[w,h]]` for a CGRect.
#[test]
fn aLayoutSavedByTheSwiftAppLoads() {
    let saved = r#"{
      "root": {"split": {"_0": {
        "id": "root", "axis": "horizontal", "fractions": [0.25, 0.75],
        "children": [
          {"group": {"_0": {"id": "left", "panels": ["threads"], "selected": "threads"}}},
          {"split": {"_0": {
            "id": "right", "axis": "vertical", "fractions": [0.5, 0.5],
            "children": [
              {"group": {"_0": {"id": "hl", "panels": ["highlights", "patches"], "selected": "patches"}}},
              {"group": {"_0": {"id": "cx", "panels": ["convex"], "selected": "convex"}}}
            ]
          }}}
        ]
      }}},
      "floating": [
        {"panel": "terminal", "frame": [[10, 20], [300, 400]], "home": {"edge": {"group": "cx", "_1": "bottom"}}, "share": 0.4},
        {"panel": "files", "frame": [[1, 2], [3, 4]], "home": {"window": {"_0": "right"}}},
        {"panel": "worktrees", "frame": [[5, 6], [7, 8]], "home": {"tab": {"group": "hl"}}}
      ],
      "hiddenTitles": ["threads"]
    }"#;
    let layout: DockLayout = serde_json::from_str(saved).expect("the Swift shape loads");
    assert_eq!(layout.hidden_titles.iter().cloned().collect::<Vec<_>>(), vec!["threads".to_string()]);
    assert_eq!(layout.summary(), "row[threads (no title) 25% | column[highlights+patches* 50% / convex 50%] 75%] + window[terminal] + window[files] + window[worktrees]");
    assert_eq!(layout.floating[0].frame, Rect::new(10., 20., 300., 400.));
    assert_eq!(layout.floating[0].home, Some(DockDrop::edge("cx", DockEdge::Bottom)));
    assert_eq!(layout.floating[0].share, Some(0.4));
    assert_eq!(layout.floating[1].home, Some(DockDrop::window(DockEdge::Right)));
    assert_eq!(layout.floating[2].home, Some(DockDrop::tab("hl")));
    // What we write reads back the same, and is still Swift's shape.
    let written = serde_json::to_string(&layout).expect("encodes");
    assert_eq!(serde_json::from_str::<DockLayout>(&written).expect("decodes"), layout);
    assert!(written.contains(r#""root":{"split":{"_0":"#), "{written}");
    assert!(written.contains(r#""frame":[[10.0,20.0],[300.0,400.0]]"#), "{written}");
    assert!(written.contains(r#""home":{"edge":{"_1":"bottom","group":"cx"}}"#) || written.contains(r#""home":{"edge":{"group":"cx","_1":"bottom"}}"#), "{written}");
}

// BarDropTests: dragging a panel over another group's bar joins it as a tab,
// shown as a tab after the titles there (Mike, 28 Oct).

fn barController() -> DockController {
    let mut c = DockController::new(DockLayout::new(
        DockNode::Split(DockSplit::new("root", SplitAxis::Horizontal, vec![g("mid", &["transcript"]), g("right", &["highlights", "worktrees"])], None)),
        Vec::<String>::new(),
        Vec::new(),
    ));
    c.bounds = Rect::new(0., 0., 1000., 800.);
    c.frames = HashMap::from([("mid".to_string(), Rect::new(0., 0., 600., 800.)), ("right".to_string(), Rect::new(600., 0., 400., 800.))]);
    c.bars = HashMap::from([("mid".to_string(), Rect::new(0., 0., 600., 32.)), ("right".to_string(), Rect::new(600., 0., 400., 32.))]);
    c.bar_space_starts = HashMap::from([("mid".to_string(), 120.), ("right".to_string(), 820.)]);
    c
}

#[test]
fn overAnotherGroupsBarItJoinsAsATab() {
    let target = barController().target_for("worktrees", Point::new(300., 16.)).expect("a target");
    assert_eq!(target.drop, Some(DockDrop::tab("mid")));
    assert!(target.in_bar);
    // The tab is drawn after the title, and the panel lands in the whole group.
    assert_eq!(target.preview.min_x(), 124.);
    assert_eq!(target.land, Some(Rect::new(0., 0., 600., 800.)));
}

#[test]
fn theWindowsTopEdgeStillDocksAlongTheVeryTop() {
    assert_eq!(barController().target_for("worktrees", Point::new(300., 2.)).and_then(|t| t.drop), Some(DockDrop::window(DockEdge::Top)));
}

#[test]
fn belowTheBarTheGroupsOwnTargetsAreBack() {
    let target = barController().target_for("worktrees", Point::new(300., 60.)).expect("a target");
    assert!(!target.in_bar);
}

// PanelAtPointTests: Threadbear's activity log names the panel a click was in.

#[test]
fn aClickIsInTheFrontPanelOfTheGroupUnderIt() {
    let mut c = DockController::new(DockLayout::new(
        DockNode::Split(DockSplit::new(
            "root",
            SplitAxis::Horizontal,
            vec![g("mid", &["transcript"]), gs("right", &["highlights", "worktrees"], "worktrees")],
            None,
        )),
        Vec::<String>::new(),
        Vec::new(),
    ));
    c.frames = HashMap::from([("mid".to_string(), Rect::new(0., 0., 600., 800.)), ("right".to_string(), Rect::new(600., 0., 400., 800.))]);
    // Swift's test passes window points, which go up from the bottom, as
    // (300, 700) and (700, 100) in an 800-tall window; on GPUI a window's
    // points go down from the top, so here they're (300, 100) and (700, 700).
    assert_eq!(c.panel_at(Point::new(300., 100.), DockWindow::Main), Some("transcript".to_string()));
    assert_eq!(c.panel_at(Point::new(700., 700.), DockWindow::Main), Some("worktrees".to_string()));
    assert_eq!(c.panel_at(Point::new(10., 10.), DockWindow::Other), None);
}

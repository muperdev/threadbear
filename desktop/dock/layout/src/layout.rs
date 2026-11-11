// BearDock's layout tree, ported line for line from
// dock/Sources/BearDock/DockLayout.swift. Pure logic, no UI. The JSON it reads
// and writes is the shape Swift's Codable uses, so a layout saved by the Mac
// app still loads here.
use crate::geometry::Rect;
use serde::de::Error as _;
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// A panel's id. The app decides what each one shows; the layout only says
/// where it sits.
pub type PanelId = String;

/// Which way a split lays out its children: side by side, or one above the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

/// A side of a group, or of the whole window, to dock against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockEdge {
    Left,
    Right,
    Top,
    Bottom,
}

impl DockEdge {
    pub const ALL: [DockEdge; 4] = [DockEdge::Left, DockEdge::Right, DockEdge::Top, DockEdge::Bottom];

    pub fn axis(self) -> SplitAxis {
        if self == DockEdge::Left || self == DockEdge::Right {
            SplitAxis::Horizontal
        } else {
            SplitAxis::Vertical
        }
    }

    /// Left and top put the new panel first in the split.
    pub fn is_leading(self) -> bool {
        self == DockEdge::Left || self == DockEdge::Top
    }
}

/// One or more panels in the same space, as tabs when there are several.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockGroup {
    pub id: String,
    pub panels: Vec<PanelId>,
    pub selected: PanelId,
}

impl DockGroup {
    pub fn new(id: impl Into<String>, panels: Vec<PanelId>, selected: Option<PanelId>) -> Self {
        let selected = selected.or_else(|| panels.first().cloned()).unwrap_or_default();
        Self { id: id.into(), panels, selected }
    }
}

/// Children laid out along one axis, each taking a fraction of the space.
/// Fractions always add up to 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockSplit {
    pub id: String,
    pub axis: SplitAxis,
    pub children: Vec<DockNode>,
    pub fractions: Vec<f64>,
}

impl DockSplit {
    pub fn new(id: impl Into<String>, axis: SplitAxis, children: Vec<DockNode>, fractions: Option<Vec<f64>>) -> Self {
        let fractions = fractions.unwrap_or_else(|| vec![1. / children.len().max(1) as f64; children.len()]);
        Self { id: id.into(), axis, children, fractions }
    }
}

/// The layout is a tree: splits all the way down to groups of panels. Any
/// depth is allowed; `normalised()` keeps it tidy (no empty groups, no
/// one-child splits, no split directly inside another along the same axis).
#[derive(Clone, Debug, PartialEq)]
pub enum DockNode {
    Group(DockGroup),
    Split(DockSplit),
}

impl DockNode {
    pub fn id(&self) -> &str {
        match self {
            DockNode::Group(g) => &g.id,
            DockNode::Split(s) => &s.id,
        }
    }
}

/// Where a dragged panel lands.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DockDrop {
    /// Beside a group, splitting the space it had.
    Edge { group: String, edge: DockEdge },
    /// Into a group, as another tab.
    Tab { group: String },
    /// Trading places with the panel showing in a group.
    Swap { group: String },
    /// Along a whole side of the window, the full height or width.
    Window(DockEdge),
}

impl DockDrop {
    pub fn edge(group: impl Into<String>, edge: DockEdge) -> Self {
        DockDrop::Edge { group: group.into(), edge }
    }
    pub fn tab(group: impl Into<String>) -> Self {
        DockDrop::Tab { group: group.into() }
    }
    pub fn swap(group: impl Into<String>) -> Self {
        DockDrop::Swap { group: group.into() }
    }
    pub fn window(edge: DockEdge) -> Self {
        DockDrop::Window(edge)
    }
}

/// A panel taken out of the dock into a window of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockFloat {
    pub panel: PanelId,
    /// The window's frame on screen, so it opens there again next launch.
    /// On GPUI this is a top-left-origin frame in global screen pixels (a
    /// frame the Mac app saved has AppKit's bottom-left origin; the
    /// off-screen rule recentres those that come up in the wrong place).
    pub frame: Rect,
    /// Where "Put Back" docks it: as a tab in the group it left, or along
    /// the side of the window it was nearest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<DockDrop>,
    /// For a home along a side of the window: the share of the window it
    /// had across that way, so it comes back the size it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<f64>,
}

impl DockFloat {
    pub fn new(panel: impl Into<PanelId>, frame: Rect, home: Option<DockDrop>, share: Option<f64>) -> Self {
        Self { panel: panel.into(), frame, home, share }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockLayout {
    pub root: DockNode,
    /// Panels in windows of their own, oldest first. Each is still open
    /// (`contains` says so) but in no group. Not in layouts saved before
    /// panels could have windows of their own.
    #[serde(default)]
    pub floating: Vec<DockFloat>,
    /// Panels that go without a title bar while they're on their own (the
    /// thread list doesn't need "Threads" over it). Kept per panel, not per
    /// place, so it follows the panel when it's moved. Not in layouts saved
    /// before titles could be hidden.
    #[serde(default)]
    pub hidden_titles: BTreeSet<PanelId>,
}

/// Fresh ids for groups and splits, like Swift's `UUID().uuidString`:
/// unique across runs, since saved layouts keep them.
pub fn default_new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("node-{t:x}-{n:x}")
}

impl DockLayout {
    pub fn new(root: DockNode, hidden_titles: impl IntoIterator<Item = PanelId>, floating: Vec<DockFloat>) -> Self {
        Self { root, floating, hidden_titles: hidden_titles.into_iter().collect() }
    }

    /// Whether a group gets its title bar. Always with tabs: they're how you
    /// switch between its panels.
    pub fn shows_header(&self, group: &DockGroup) -> bool {
        group.panels.len() > 1 || !self.hidden_titles.contains(&group.selected)
    }

    pub fn set_title_hidden(&mut self, panel: &str, hidden: bool) {
        if hidden {
            self.hidden_titles.insert(panel.to_string());
        } else {
            self.hidden_titles.remove(panel);
        }
    }

    // MARK: Reading

    /// Every panel in the layout, in tree order, then those in windows of
    /// their own.
    pub fn panels(&self) -> Vec<PanelId> {
        let mut out = Self::panels_in(&self.root);
        out.extend(self.floating.iter().map(|f| f.panel.clone()));
        out
    }

    /// Open, in the dock or a window of its own.
    pub fn contains(&self, panel: &str) -> bool {
        self.group_containing(panel).is_some() || self.is_floating(panel)
    }

    /// In a window of its own rather than the dock.
    pub fn is_floating(&self, panel: &str) -> bool {
        self.floating.iter().any(|f| f.panel == panel)
    }

    pub fn group(&self, id: &str) -> Option<DockGroup> {
        Self::groups_in(&self.root).into_iter().find(|g| g.id == id)
    }

    pub fn group_containing(&self, panel: &str) -> Option<DockGroup> {
        Self::groups_in(&self.root).into_iter().find(|g| g.panels.iter().any(|p| p == panel))
    }

    pub fn groups(&self) -> Vec<DockGroup> {
        Self::groups_in(&self.root)
    }

    /// The tree in one line, for logs and the app's dump: rows and
    /// columns with each child's share, tabs joined by "+" with the one
    /// showing starred, "(no title)" where it's hidden.
    /// `row[threads (no title) 29% | thread 47% | column[highlights+patches* 60% / convex 40%] 24%]`
    /// Panels in windows of their own follow, as `+ window[highlights]`.
    pub fn summary(&self) -> String {
        let tree = Self::summary_of(&self.root, &self.hidden_titles);
        if self.floating.is_empty() {
            tree
        } else {
            tree + &self.floating.iter().map(|f| format!(" + window[{}]", f.panel)).collect::<String>()
        }
    }

    fn summary_of(node: &DockNode, hidden: &BTreeSet<PanelId>) -> String {
        match node {
            DockNode::Group(g) => {
                let tabs: Vec<String> =
                    g.panels.iter().map(|p| if g.panels.len() > 1 && *p == g.selected { format!("{p}*") } else { p.clone() }).collect();
                let tabs = tabs.join("+");
                if g.panels.len() == 1 && hidden.contains(&g.selected) {
                    format!("{tabs} (no title)")
                } else {
                    tabs
                }
            }
            DockNode::Split(s) => {
                let parts: Vec<String> = s
                    .children
                    .iter()
                    .zip(s.fractions.iter())
                    .map(|(c, f)| format!("{} {}%", Self::summary_of(c, hidden), (f * 100.0).round() as i64))
                    .collect();
                let horizontal = s.axis == SplitAxis::Horizontal;
                format!("{}{}]", if horizontal { "row[" } else { "column[" }, parts.join(if horizontal { " | " } else { " / " }))
            }
        }
    }

    /// The group in the window's top-left corner: the first child all the
    /// way down, whichever way each split runs.
    pub fn top_leading_group_id(&self) -> String {
        let mut node = &self.root;
        while let DockNode::Split(s) = node {
            match s.children.first() {
                Some(first) => node = first,
                None => break,
            }
        }
        node.id().to_string()
    }

    /// False for a drop that would leave everything where it is, so the UI
    /// doesn't offer it.
    pub fn can_drop(&self, panel: &str, drop: &DockDrop) -> bool {
        if self.is_floating(panel) {
            // From a window of its own, back into the dock: anywhere but a
            // swap, as there's no place in the dock to swap with.
            return match drop {
                DockDrop::Edge { group: target, .. } | DockDrop::Tab { group: target } => self.group(target).is_some(),
                DockDrop::Swap { .. } => false,
                DockDrop::Window(_) => true,
            };
        }
        let Some(source) = self.group_containing(panel) else { return false };
        match drop {
            DockDrop::Edge { group: target, .. } => {
                if self.group(target).is_none() {
                    return false;
                }
                // Splitting a group by a panel it alone holds goes nowhere.
                !(*target == source.id && source.panels.len() == 1)
            }
            DockDrop::Tab { group: target } | DockDrop::Swap { group: target } => self.group(target).is_some() && *target != source.id,
            DockDrop::Window(_) => {
                // The only panel there is can't go anywhere else.
                if let DockNode::Group(g) = &self.root {
                    if g.panels.len() == 1 && g.panels[0] == panel {
                        return false;
                    }
                }
                true
            }
        }
    }

    /// Whether a panel in the dock can go into a window of its own: not the
    /// last one there, as the main window would have nothing in it.
    pub fn can_float(&self, panel: &str) -> bool {
        if self.group_containing(panel).is_none() {
            return false;
        }
        if let DockNode::Group(g) = &self.root {
            if g.panels.len() == 1 && g.panels[0] == panel {
                return false;
            }
        }
        true
    }

    // MARK: Changing

    /// Takes a panel out of the dock into a window of its own at `frame`
    /// (on screen). For one that's already in a window, just moves it.
    pub fn float(&mut self, panel: &str, frame: Rect, home: Option<DockDrop>, share: Option<f64>) {
        if let Some(i) = self.floating.iter().position(|f| f.panel == panel) {
            self.floating[i].frame = frame;
            return;
        }
        if !self.can_float(panel) {
            return;
        }
        let Some(rest) = Self::removing(panel, &self.root) else { return };
        self.root = rest;
        self.floating.push(DockFloat::new(panel, frame, home, share));
    }

    /// Moves a panel already in the layout. Does nothing for a drop
    /// `can_drop` refuses. New groups or splits get fresh ids; `move_panel_with`
    /// lets a test pick them.
    pub fn move_panel(&mut self, panel: &str, drop: DockDrop) {
        self.move_panel_with(panel, drop, &mut default_new_id)
    }

    /// `move_panel` with `new_id` naming any group or split this makes.
    pub fn move_panel_with(&mut self, panel: &str, drop: DockDrop, new_id: &mut dyn FnMut() -> String) {
        if !self.can_drop(panel, &drop) {
            return;
        }
        if let Some(i) = self.floating.iter().position(|f| f.panel == panel) {
            let float = self.floating.remove(i);
            // Back home along a side: the size it left at.
            let share = if float.home.as_ref() == Some(&drop) { float.share } else { None };
            self.insert(panel, &drop, share, new_id);
            return;
        }
        if let DockDrop::Swap { group: target } = &drop {
            self.swap_panels(panel, target);
            return;
        }
        let Some(rest) = Self::removing(panel, &self.root) else { return };
        self.root = rest;
        self.insert(panel, &drop, None, new_id);
    }

    /// Puts a panel that isn't in the layout yet (a new one, or one that was
    /// closed) somewhere. A panel already in it is moved instead.
    pub fn add(&mut self, panel: &str, drop: DockDrop) {
        self.add_with(panel, drop, &mut default_new_id)
    }

    pub fn add_with(&mut self, panel: &str, drop: DockDrop, new_id: &mut dyn FnMut() -> String) {
        if self.contains(panel) {
            self.move_panel_with(panel, drop, new_id);
        } else {
            self.insert(panel, &drop, None, new_id);
        }
    }

    /// Takes a panel out, closing its window if it has one of its own. The
    /// last panel in the dock can't be closed: there'd be no layout left.
    pub fn close(&mut self, panel: &str) {
        if self.is_floating(panel) {
            self.floating.retain(|f| f.panel != panel);
            return;
        }
        let Some(rest) = Self::removing(panel, &self.root) else { return };
        self.root = rest;
    }

    /// Shows a panel that's a tab in its group.
    pub fn select(&mut self, panel: &str) {
        let root = Self::map_groups(&self.root, &mut |g: &DockGroup| {
            let mut g = g.clone();
            if g.panels.iter().any(|p| p == panel) {
                g.selected = panel.to_string();
            }
            g
        });
        self.root = root;
    }

    /// From a divider being dragged. Keeps the sum at 1.
    pub fn set_fractions(&mut self, split: &str, fractions: &[f64]) {
        let root = Self::map_splits(&self.root, &mut |s: &DockSplit| {
            let mut s = s.clone();
            if s.id == split && s.children.len() == fractions.len() {
                s.fractions = Self::normalised_fractions(fractions);
            }
            s
        });
        self.root = root;
    }

    // MARK: Internals

    /// `share`: for a drop along a side of the window, how much of it the
    /// panel takes; a quarter if nil.
    fn insert(&mut self, panel: &str, drop: &DockDrop, share: Option<f64>, new_id: &mut dyn FnMut() -> String) {
        let fresh = DockNode::Group(DockGroup::new(new_id(), vec![panel.to_string()], None));
        match drop {
            DockDrop::Edge { group: target, edge } => {
                self.root = Self::inserting(&fresh, target, *edge, &self.root, new_id);
            }
            DockDrop::Tab { group: target } => {
                self.root = Self::map_groups(&self.root, &mut |g: &DockGroup| {
                    let mut g = g.clone();
                    if g.id == *target {
                        g.panels.push(panel.to_string());
                        g.selected = panel.to_string();
                    }
                    g
                });
            }
            DockDrop::Swap { .. } => {}
            DockDrop::Window(edge) => {
                // A new outer child takes a quarter (or its old share); the
                // rest keeps its proportions.
                let take = share.unwrap_or(0.25).max(0.1).min(0.9);
                if let DockNode::Split(s) = &self.root {
                    if s.axis == edge.axis() {
                        let mut s = s.clone();
                        s.fractions = s.fractions.iter().map(|f| f * (1. - take)).collect();
                        if edge.is_leading() {
                            s.children.insert(0, fresh);
                            s.fractions.insert(0, take);
                        } else {
                            s.children.push(fresh);
                            s.fractions.push(take);
                        }
                        self.root = DockNode::Split(s);
                        self.root = Self::normalised(&self.root);
                        return;
                    }
                }
                let old_root = self.root.clone();
                let (children, fractions) =
                    if edge.is_leading() { (vec![fresh, old_root], vec![take, 1. - take]) } else { (vec![old_root, fresh], vec![1. - take, take]) };
                self.root = DockNode::Split(DockSplit::new(new_id(), edge.axis(), children, Some(fractions)));
            }
        }
        self.root = Self::normalised(&self.root);
    }

    fn swap_panels(&mut self, panel: &str, target: &str) {
        let Some(source) = self.group_containing(panel) else { return };
        let Some(other) = self.group(target).map(|g| g.selected.clone()) else { return };
        self.root = Self::map_groups(&self.root, &mut |g: &DockGroup| {
            let mut g = g.clone();
            if g.id == source.id {
                g.panels = g.panels.iter().map(|p| if p == panel { other.clone() } else { p.clone() }).collect();
                if g.selected == panel {
                    g.selected = other.clone();
                }
            } else if g.id == target {
                g.panels = g.panels.iter().map(|p| if *p == other { panel.to_string() } else { p.clone() }).collect();
                g.selected = panel.to_string();
            }
            g
        });
    }

    fn inserting(fresh: &DockNode, target: &str, edge: DockEdge, node: &DockNode, new_id: &mut dyn FnMut() -> String) -> DockNode {
        match node {
            DockNode::Group(g) => {
                // Reached the target without a parent split along this axis to join.
                if g.id != target {
                    return node.clone();
                }
                let children = if edge.is_leading() { vec![fresh.clone(), node.clone()] } else { vec![node.clone(), fresh.clone()] };
                DockNode::Split(DockSplit::new(new_id(), edge.axis(), children, None))
            }
            DockNode::Split(s) => {
                let mut s = s.clone();
                // The target is a direct child and this split runs the same way:
                // join it as a sibling, sharing the target's space, rather than
                // nesting another split.
                if s.axis == edge.axis() {
                    if let Some(i) = s.children.iter().position(|c| c.id() == target) {
                        let half = s.fractions[i] / 2.;
                        s.fractions[i] = half;
                        let at = if edge.is_leading() { i } else { i + 1 };
                        s.children.insert(at, fresh.clone());
                        s.fractions.insert(at, half);
                        return DockNode::Split(s);
                    }
                }
                s.children = s.children.iter().map(|c| Self::inserting(fresh, target, edge, c, new_id)).collect();
                DockNode::Split(s)
            }
        }
    }

    /// The node without the panel, tidied, or nil if nothing is left.
    pub fn removing(panel: &str, node: &DockNode) -> Option<DockNode> {
        match node {
            DockNode::Group(g) => {
                let mut g = g.clone();
                let Some(i) = g.panels.iter().position(|p| p == panel) else { return Some(node.clone()) };
                g.panels.remove(i);
                if g.panels.is_empty() {
                    return None;
                }
                // The neighbour takes over, as closing a tab does in an editor.
                if g.selected == panel {
                    g.selected = g.panels[i.min(g.panels.len() - 1)].clone();
                }
                Some(DockNode::Group(g))
            }
            DockNode::Split(s) => {
                let mut children = Vec::new();
                let mut fractions = Vec::new();
                for (child, f) in s.children.iter().zip(s.fractions.iter()) {
                    if let Some(kept) = Self::removing(panel, child) {
                        children.push(kept);
                        fractions.push(*f);
                    }
                }
                if children.is_empty() {
                    return None;
                }
                let mut s = s.clone();
                s.children = children;
                s.fractions = fractions;
                Some(Self::normalised(&DockNode::Split(s)))
            }
        }
    }

    /// No one-child splits, no split inside another along the same axis,
    /// fractions adding up to 1. Applied all the way down.
    pub fn normalised(node: &DockNode) -> DockNode {
        let DockNode::Split(s) = node else { return node.clone() };
        let mut s = s.clone();
        let mut children: Vec<DockNode> = Vec::new();
        let mut fractions: Vec<f64> = Vec::new();
        let tidied: Vec<DockNode> = s.children.iter().map(Self::normalised).collect();
        for (child, f) in tidied.into_iter().zip(s.fractions.iter()) {
            match child {
                DockNode::Split(inner) if inner.axis == s.axis => {
                    // Same direction: its children become ours, scaled to its share.
                    children.extend(inner.children);
                    fractions.extend(inner.fractions.iter().map(|x| x * f));
                }
                other => {
                    children.push(other);
                    fractions.push(*f);
                }
            }
        }
        if children.len() == 1 {
            return children.remove(0);
        }
        s.children = children;
        s.fractions = Self::normalised_fractions(&fractions);
        DockNode::Split(s)
    }

    pub fn normalised_fractions(fractions: &[f64]) -> Vec<f64> {
        let sum: f64 = fractions.iter().sum();
        if !(sum > 0.) {
            return vec![1. / fractions.len().max(1) as f64; fractions.len()];
        }
        fractions.iter().map(|f| f / sum).collect()
    }

    fn map_groups(node: &DockNode, f: &mut dyn FnMut(&DockGroup) -> DockGroup) -> DockNode {
        match node {
            DockNode::Group(g) => DockNode::Group(f(g)),
            DockNode::Split(s) => {
                let mut s = s.clone();
                s.children = s.children.iter().map(|c| Self::map_groups(c, f)).collect();
                DockNode::Split(s)
            }
        }
    }

    fn map_splits(node: &DockNode, f: &mut dyn FnMut(&DockSplit) -> DockSplit) -> DockNode {
        match node {
            DockNode::Group(_) => node.clone(),
            DockNode::Split(s) => {
                let mut s = f(s);
                s.children = s.children.iter().map(|c| Self::map_splits(c, f)).collect();
                DockNode::Split(s)
            }
        }
    }

    pub fn groups_in(node: &DockNode) -> Vec<DockGroup> {
        match node {
            DockNode::Group(g) => vec![g.clone()],
            DockNode::Split(s) => s.children.iter().flat_map(Self::groups_in).collect(),
        }
    }

    pub fn panels_in(node: &DockNode) -> Vec<PanelId> {
        Self::groups_in(node).into_iter().flat_map(|g| g.panels).collect()
    }
}

// Swift's Codable writes an enum case with unnamed associated values as
// `{"case":{"_0":value}}`: kept, so a layout the Mac app saved loads here.

#[derive(Serialize)]
struct Positional<'a, T> {
    _0: &'a T,
}

impl Serialize for DockNode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(1))?;
        match self {
            DockNode::Group(g) => map.serialize_entry("group", &Positional { _0: g })?,
            DockNode::Split(split) => map.serialize_entry("split", &Positional { _0: split })?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for DockNode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Written by hand so it stays explicit that this is Swift's shape.
        #[derive(Deserialize)]
        struct GroupCase {
            #[serde(rename = "_0")]
            value: DockGroup,
        }
        #[derive(Deserialize)]
        struct SplitCase {
            #[serde(rename = "_0")]
            value: DockSplit,
        }
        #[derive(Deserialize)]
        struct Wrapper {
            group: Option<GroupCase>,
            split: Option<SplitCase>,
        }
        let w = Wrapper::deserialize(d)?;
        match (w.group, w.split) {
            (Some(g), None) => Ok(DockNode::Group(g.value)),
            (None, Some(s)) => Ok(DockNode::Split(s.value)),
            _ => Err(D::Error::custom("a dock node has exactly one of group or split")),
        }
    }
}

impl Serialize for DockDrop {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde_json::json;
        let v = match self {
            DockDrop::Edge { group, edge } => json!({ "edge": { "group": group, "_1": edge } }),
            DockDrop::Tab { group } => json!({ "tab": { "group": group } }),
            DockDrop::Swap { group } => json!({ "swap": { "group": group } }),
            DockDrop::Window(edge) => json!({ "window": { "_0": edge } }),
        };
        v.serialize(s)
    }
}

impl<'de> Deserialize<'de> for DockDrop {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde_json::Value;
        let v = Value::deserialize(d)?;
        let (key, inner) = v.as_object().and_then(|o| o.iter().next()).ok_or_else(|| D::Error::custom("a dock drop is an object with one key"))?;
        let group = |inner: &Value| -> Result<String, D::Error> {
            inner.get("group").and_then(Value::as_str).map(str::to_string).ok_or_else(|| D::Error::custom("this dock drop needs a group"))
        };
        let edge = |value: Option<&Value>| -> Result<DockEdge, D::Error> {
            let value = value.ok_or_else(|| D::Error::custom("this dock drop needs an edge"))?;
            serde_json::from_value(value.clone()).map_err(D::Error::custom)
        };
        Ok(match key.as_str() {
            "edge" => DockDrop::Edge { group: group(inner)?, edge: edge(inner.get("_1").or_else(|| inner.get("edge")))? },
            "tab" => DockDrop::Tab { group: group(inner)? },
            "swap" => DockDrop::Swap { group: group(inner)? },
            "window" => DockDrop::Window(edge(inner.get("_0"))?),
            other => return Err(D::Error::custom(format!("unknown dock drop {other}"))),
        })
    }
}

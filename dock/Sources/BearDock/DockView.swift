import AppKit
import SwiftUI

/// What the app gives the dock for each panel.
public struct DockContent {
    public var title: (PanelID) -> String
    /// Shown on hovering the title (a file's full path, say). Nil: none.
    public var titleTip: (PanelID) -> String?
    public var symbol: (PanelID) -> String?
    /// A picture drawn in colour in place of the symbol (a tool's own icon).
    public var image: (PanelID) -> NSImage?
    /// A small mark over the bottom corner of the tab's icon (where the
    /// panel came from, say). Nil: none.
    public var badge: (PanelID) -> AnyView?
    public var view: (PanelID) -> AnyView
    /// Drawn in place of the title when the panel is on its own (the
    /// thread's name and folder over the transcript, say). Nil: the title.
    public var header: (PanelID) -> AnyView?
    /// Buttons at the end of the panel's bar while it's showing.
    public var accessory: (PanelID) -> AnyView?
    /// False for a panel the app always keeps (the transcript): no ×, no Close.
    public var closable: (PanelID) -> Bool
    /// False for a panel with nothing to show right now (the selected
    /// thread has no such panel): its tab is left out and another of its
    /// group's shows, without changing the layout, so it's back when it
    /// has something again.
    public var available: (PanelID) -> Bool

    public init(
        title: @escaping (PanelID) -> String,
        titleTip: @escaping (PanelID) -> String? = { _ in nil },
        symbol: @escaping (PanelID) -> String? = { _ in nil },
        image: @escaping (PanelID) -> NSImage? = { _ in nil },
        badge: @escaping (PanelID) -> AnyView? = { _ in nil },
        header: @escaping (PanelID) -> AnyView? = { _ in nil },
        accessory: @escaping (PanelID) -> AnyView? = { _ in nil },
        closable: @escaping (PanelID) -> Bool = { _ in true },
        available: @escaping (PanelID) -> Bool = { _ in true },
        view: @escaping (PanelID) -> AnyView
    ) {
        self.available = available
        self.title = title
        self.titleTip = titleTip
        self.symbol = symbol
        self.image = image
        self.badge = badge
        self.header = header
        self.accessory = accessory
        self.closable = closable
        self.view = view
    }
}

/// How the dock fits the app around it.
public struct DockOptions {
    public var headerHeight: CGFloat = 32
    public var titleSize: CGFloat = 12
    /// In a window without a title bar: room left at the start of the
    /// top-left panel's bar for the traffic lights. 0 with a title bar.
    public var trafficLights: CGFloat = 0
    /// In a window without a title bar: the strip beside the traffic lights
    /// moves the window, as a title bar would. Only that strip: Mike reached
    /// for a panel's bar to move the panel and got the window (28 Oct), so
    /// every other bar moves its panel, empty part and all.
    public var cornerMovesWindow = false
    /// In a window without a title bar: a plain drag on a panel's bar
    /// (title, tabs or empty part) moves the window, as a title bar's
    /// would. Picking a panel up always takes a press held for a moment
    /// first (HoldDrag), with or without this. Mike kept getting the panel
    /// when he went to move the window by the thread's bar (28 Oct), and
    /// the panel when he wanted the window before that, so the bar does both.
    public var barsMoveWindow = false
    /// The app's tooltips, for an app whose `.help` doesn't show.
    public var tip: @MainActor (AnyView, String) -> AnyView = { view, text in AnyView(view.help(text)) }
    /// Wraps a panel in a window of its own, to give it what the app's
    /// main window gives its views (its model in the environment, say),
    /// as a new window doesn't inherit any of that.
    public var windowContent: @MainActor (AnyView) -> AnyView = { $0 }
    /// Called on a panel's window of its own as it's made, before it shows.
    public var prepareWindow: @MainActor (NSWindow) -> Void = { _ in }

    public init() {}
}

enum DockSpace {
    static let name = "BearDock"
}

/// System colours, so the dock follows light and dark like the app around it.
enum DockTheme {
    /// The narrowest a panel's content is laid out (GroupView).
    static let minContentWidth: CGFloat = 160
    static let panel = Color(nsColor: .windowBackgroundColor)
    /// Shows through the 1 pt gaps between panels, as the hairlines.
    static let hairline = Color(nsColor: .separatorColor)
    static let headerText = Color.primary
    static let secondary = Color.secondary
    static let faint = Color(nsColor: .tertiaryLabelColor)
    static let accent = Color.accentColor
    static let gap: CGFloat = 1
}

/// The whole docking area: the layout's tree of splits and groups, with the
/// drag's ghost, drop preview and compass drawn over it.
public struct DockView: View {
    let controller: DockController
    let content: DockContent
    let options: DockOptions
    @Namespace private var space

    public init(controller: DockController, options: DockOptions = DockOptions(), content: DockContent) {
        self.controller = controller
        self.options = options
        self.content = content
    }

    public init<C: View>(
        controller: DockController,
        options: DockOptions = DockOptions(),
        title: @escaping (PanelID) -> String,
        symbol: @escaping (PanelID) -> String? = { _ in nil },
        @ViewBuilder content: @escaping (PanelID) -> C
    ) {
        self.init(controller: controller, options: options, content: DockContent(title: title, symbol: symbol, view: { AnyView(content($0)) }))
    }

    public var body: some View {
        NodeView(node: controller.layout.root)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(DockTheme.hairline)
            .background(DockTheme.panel)
            .overlay(alignment: .topLeading) {
                DragOverlay()
                    .allowsHitTesting(false)
            }
            .coordinateSpace(.named(DockSpace.name))
            .onGeometryChange(for: CGRect.self) { CGRect(origin: .zero, size: $0.size) } action: { controller.bounds = $0 }
            .background(DockAnchor(controller: controller, locksWindow: options.cornerMovesWindow))
            // Panels in windows of their own: opened (again, at launch),
            // moved and closed as the layout says.
            .onAppear { syncWindows() }
            .onChange(of: controller.layout.floating) { syncWindows() }
            // A floating panel coming and going with what's on show (a
            // thread's patch panel): reading `available` here is what makes
            // the view notice.
            .onChange(of: controller.layout.floating.map { content.available($0.panel) }) { syncWindows() }
            .environment(controller)
            .environment(\.dockContent, content)
            .environment(\.dockOptions, options)
            .environment(\.dockNamespace, space)
    }

    private func syncWindows() {
        controller.windows.content = content
        controller.windows.options = options
        controller.windows.sync()
    }
}

extension EnvironmentValues {
    // Written out rather than @Entry: SwiftUI's macros ship with Xcode
    // only, and this builds with the command line tools too.
    var dockContent: DockContent {
        get { self[DockContentKey.self] }
        set { self[DockContentKey.self] = newValue }
    }
    var dockOptions: DockOptions {
        get { self[DockOptionsKey.self] }
        set { self[DockOptionsKey.self] = newValue }
    }
    var dockNamespace: Namespace.ID? {
        get { self[DockNamespaceKey.self] }
        set { self[DockNamespaceKey.self] = newValue }
    }
    /// True for a panel's content drawn in the ghost that follows a drag:
    /// a second copy of what's in the panel. Content that hands out one
    /// shared AppKit view (a transcript, a web view) should draw a picture
    /// of it here instead, or the ghost takes the view and the panel is
    /// left blank once the ghost goes.
    public var isDockGhost: Bool {
        get { self[IsDockGhostKey.self] }
        set { self[IsDockGhostKey.self] = newValue }
    }
}

private struct DockContentKey: EnvironmentKey {
    static var defaultValue: DockContent { DockContent(title: { $0 }, view: { _ in AnyView(EmptyView()) }) }
}

private struct DockOptionsKey: EnvironmentKey {
    static var defaultValue: DockOptions { DockOptions() }
}

private struct DockNamespaceKey: EnvironmentKey {
    static var defaultValue: Namespace.ID? { nil }
}

private struct IsDockGhostKey: EnvironmentKey {
    static var defaultValue: Bool { false }
}

/// A tab title's hover tip, when its panel has one.
private struct TitleTip: ViewModifier {
    let text: String?
    let options: DockOptions

    func body(content: Content) -> some View {
        if let text { content.dockTip(text, options) } else { content }
    }
}

extension View {
    @MainActor func dockTip(_ text: String, _ options: DockOptions) -> AnyView {
        options.tip(AnyView(self), text)
    }
}

/// A split or a group. AnyView is where the tree recurses.
struct NodeView: View {
    let node: DockNode

    var body: some View {
        switch node {
        case .group(let g): AnyView(GroupView(group: g))
        case .split(let s): AnyView(SplitView(split: s))
        }
    }
}

// MARK: Splits

struct SplitView: View {
    let split: DockSplit
    @Environment(DockController.self) private var controller
    @Environment(\.dockContent) private var content
    /// The fractions when a divider drag started.
    @State private var startFractions: [Double]?
    /// The divider being dragged, kept lit while the pointer runs ahead of it.
    @State private var dragging: Int?

    var body: some View {
        // Children with nothing to show right now (a thread's patch panel,
        // moved to a side of its own, while a thread without it is on show)
        // are left out and their room shared among the rest, as unavailable
        // tabs are within a group. The layout keeps them, so they're back
        // when there's something to show again.
        let shown = Self.shownIndices(split, content.available)
        let fractions = Self.normalized(shown.map { split.fractions[$0] })
        SplitLayout(axis: split.axis, fractions: fractions, gap: DockTheme.gap) {
            ForEach(shown, id: \.self) { i in
                NodeView(node: split.children[i])
            }
        }
        .overlay {
            GeometryReader { geo in
                let length = split.axis == .horizontal ? geo.size.width : geo.size.height
                let available = length - DockTheme.gap * CGFloat(shown.count - 1)
                ForEach(0..<max(shown.count - 1, 0), id: \.self) { i in
                    let before = fractions.prefix(i + 1).reduce(0, +)
                    let at = CGFloat(before) * available + CGFloat(i) * DockTheme.gap + DockTheme.gap / 2
                    DividerHandle(axis: split.axis, dragging: dragging == i)
                        .position(split.axis == .horizontal
                                  ? CGPoint(x: at, y: geo.size.height / 2)
                                  : CGPoint(x: geo.size.width / 2, y: at))
                        .gesture(
                            DragGesture(minimumDistance: 1)
                                .onChanged { value in
                                    let start = startFractions ?? split.fractions
                                    if startFractions == nil, ProcessInfo.processInfo.environment["BEARDOCK_LOG"] != nil { print("divider \(split.id) \(i) from \(value.startLocation)") }
                                    if startFractions == nil { startFractions = start; dragging = i }
                                    // The pointer runs ahead of the strip, over views that set their own.
                                    DividerHandle.cursor(split.axis).set()
                                    let t = split.axis == .horizontal ? value.translation.width : value.translation.height
                                    let visible = Self.normalized(shown.map { start[$0] })
                                    let moved = Self.resized(visible, at: i, by: t / max(available, 1), min: 80 / max(available, 1))
                                    controller.layout.setFractions(split: split.id, Self.merged(moved, into: start, at: shown))
                                }
                                .onEnded { _ in startFractions = nil; dragging = nil; NSCursor.arrow.set() }
                        )
                }
            }
        }
    }

    /// The children that have a panel with something to show, by index.
    /// All of them if none has, as a group shows all its tabs then.
    static func shownIndices(_ split: DockSplit, _ available: (PanelID) -> Bool) -> [Int] {
        let shown = split.children.indices.filter { hasAvailable(split.children[$0], available) }
        return shown.isEmpty ? Array(split.children.indices) : shown
    }

    static func hasAvailable(_ node: DockNode, _ available: (PanelID) -> Bool) -> Bool {
        switch node {
        case .group(let g): g.panels.contains(where: available)
        case .split(let s): s.children.contains { hasAvailable($0, available) }
        }
    }

    static func normalized(_ f: [Double]) -> [Double] {
        let total = f.reduce(0, +)
        return total > 0 ? f.map { $0 / total } : f.map { _ in 1 / Double(max(f.count, 1)) }
    }

    /// Fractions of the shown children (summing to 1), put back into the
    /// whole split's: the hidden ones keep theirs, the shown ones share
    /// what's left in the new proportions.
    static func merged(_ visible: [Double], into all: [Double], at shown: [Int]) -> [Double] {
        var out = all
        let room = shown.reduce(0) { $0 + all[$1] }
        for (k, i) in shown.enumerated() { out[i] = visible[k] * room }
        return out
    }

    /// Moves the boundary after child i, keeping both sides at least `min`.
    static func resized(_ f: [Double], at i: Int, by delta: CGFloat, min: CGFloat) -> [Double] {
        var f = f
        let pair = f[i] + f[i + 1]
        let lo = Double(min), hi = pair - Double(min)
        guard lo < hi else { return f }
        f[i] = Swift.min(Swift.max(f[i] + Double(delta), lo), hi)
        f[i + 1] = pair - f[i]
        return f
    }
}

/// Lays children out along an axis by fraction, with a gap between each.
struct SplitLayout: Layout {
    var axis: SplitAxis
    var fractions: [Double]
    var gap: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        proposal.replacingUnspecifiedDimensions()
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let horizontal = axis == .horizontal
        let length = horizontal ? bounds.width : bounds.height
        let available = max(length - gap * CGFloat(subviews.count - 1), 0)
        var offset: CGFloat = 0
        for (i, sub) in subviews.enumerated() {
            let f = i < fractions.count ? fractions[i] : 0
            let size = (available * CGFloat(f)).rounded()
            let rect = horizontal
                ? CGRect(x: bounds.minX + offset, y: bounds.minY, width: size, height: bounds.height)
                : CGRect(x: bounds.minX, y: bounds.minY + offset, width: bounds.width, height: size)
            sub.place(at: rect.origin, proposal: ProposedViewSize(rect.size))
            offset += size + gap
        }
    }
}

/// The invisible strip over a hairline that resizes the panels either side.
/// 14 pt wide and lit in the accent almost at once (Mike, 30 Oct: the old
/// 9 pt strip with a faint, late line was fiddly to find). The short delay
/// stays so the pointer passing over it on the way somewhere doesn't flash.
private struct DividerHandle: View {
    let axis: SplitAxis
    var dragging = false
    @State private var hovering = false

    var body: some View {
        let lit = hovering || dragging
        Rectangle()
            .fill(DockTheme.accent.opacity(lit ? 0.85 : 0))
            .frame(width: axis == .horizontal ? 3 : nil, height: axis == .vertical ? 3 : nil)
            .frame(width: axis == .horizontal ? 14 : nil, height: axis == .vertical ? 14 : nil)
            .frame(maxWidth: axis == .vertical ? .infinity : nil, maxHeight: axis == .horizontal ? .infinity : nil)
            .contentShape(Rectangle())
            .animation(.easeOut(duration: 0.1).delay(lit ? 0.05 : 0), value: lit)
            // Set on every move, not pushed once on the way in: the views
            // under the strip (the sidebar's scroller, a transcript's text)
            // set their own cursor as the pointer moves and won, so the line
            // lit with an arrow showing (Mike, 30 Oct). As TextKit's hand.
            .onContinuousHover { phase in
                switch phase {
                case .active:
                    hovering = true
                    Self.cursor(axis).set()
                case .ended:
                    hovering = false
                    if !dragging { NSCursor.arrow.set() }
                }
            }
    }

    static func cursor(_ axis: SplitAxis) -> NSCursor {
        axis == .horizontal ? .resizeLeftRight : .resizeUpDown
    }
}

// MARK: Groups

struct GroupView: View {
    let group: DockGroup
    @Environment(DockController.self) private var controller
    @Environment(\.dockContent) private var content
    @Environment(\.dockOptions) private var options
    @Environment(\.dockNamespace) private var space

    var body: some View {
        // The tabs with something to show; the layout keeps the rest.
        let group = Self.visible(group, content.available)
        let lifted = controller.drag.map { $0.lifted && $0.panel == group.selected } ?? false
        // Where a panel just landed stays empty while the ghost flies in,
        // then fades up under it.
        let landing = controller.landing?.panel == group.selected

        let header = controller.layout.showsHeader(group)
        // Under the traffic lights, in a window without a title bar: the
        // bar stays (empty, if the title's hidden) so they've somewhere to sit.
        let corner = options.trafficLights > 0 && Self.topLeadingGroupID(controller.layout.root, content.available) == group.id

        VStack(spacing: 0) {
            if header {
                GroupHeader(group: group, leading: corner ? options.trafficLights : 0, movesWindow: corner)
                    // Exactly the group's width: tabs that don't fit run off
                    // its trailing edge, rather than widening the group past
                    // its slot, which then hung off both sides, centred.
                    .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                    .clipped()
                    .transition(.move(edge: .top).combined(with: .opacity))
            } else if corner {
                CornerBar(panel: group.selected)
            }
            content.view(group.selected)
                // Never laid out narrower than this: in a thinner slot the
                // panel keeps its layout, pinned to the leading edge, and
                // its far side is cut off, rather than wrapping a letter a line.
                .frame(minWidth: DockTheme.minContentWidth, maxWidth: .infinity, maxHeight: .infinity)
                .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity, alignment: .leading)
                .clipped()
                .contentShape(Rectangle())
                // A plain .gesture, the lowest priority: any drag the panel's
                // own content handles (reordering rows, a slider, a text
                // selection, a web view) wins, so only a held press on its
                // background moves the panel. `dockDragDisabled()` stops it
                // for a region that has nothing of its own.
                .modifier(HoldDrag(panel: group.selected, movesWindow: false, fromPress: false))
                // The same menu as the title's, on the background, so a hidden
                // title can be brought back from anywhere empty. Outermost, so
                // content with a menu of its own keeps it.
                .contextMenu { PanelMenu(panel: group.selected, tabbed: group.panels.count > 1) }
                .opacity(lifted ? 0.18 : 1)
                .scaleEffect(lifted ? 0.985 : 1)
        }
        .background(DockTheme.panel)
        .overlay(alignment: .top) {
            if !header && !corner {
                HiddenTitleStrip(panel: group.selected)
                    .transition(.opacity)
            }
        }
        .overlay {
            if lifted {
                // The hole it left behind.
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(style: StrokeStyle(lineWidth: 1, dash: [5, 4]))
                    .foregroundStyle(DockTheme.faint)
                    .padding(6)
                    .transition(.opacity)
            }
        }
        .opacity(landing ? 0 : 1)
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .named(DockSpace.name)) } action: { controller.frames[group.id] = $0 }
        .modifier(MatchedGroup(id: group.id, space: space))
    }
}

extension GroupView {
    /// The group in the window's top-leading corner as drawn, where the
    /// traffic lights sit: a side left out for having nothing to show
    /// (SplitView) passes the corner on to the next.
    static func topLeadingGroupID(_ root: DockNode, _ available: (PanelID) -> Bool) -> String {
        var node = root
        while case .split(let s) = node, let first = SplitView.shownIndices(s, available).first { node = s.children[first] }
        return node.id
    }

    /// The group as drawn: without its unavailable panels, showing the
    /// selected one if it's available, else the first that is. All of
    /// them, if none is.
    static func visible(_ group: DockGroup, _ available: (PanelID) -> Bool) -> DockGroup {
        let panels = group.panels.filter(available)
        guard !panels.isEmpty else { return group }
        return DockGroup(id: group.id, panels: panels, selected: panels.contains(group.selected) ? group.selected : panels[0])
    }
}

/// Lets a group glide to its new place when it moves to another split.
private struct MatchedGroup: ViewModifier {
    let id: String
    let space: Namespace.ID?

    func body(content: Content) -> some View {
        if let space {
            content.matchedGeometryEffect(id: id, in: space)
        } else {
            content
        }
    }
}

/// A group's title, or its tabs when it has several, and the showing
/// panel's buttons. A title, a tab or the rest of the bar drags its panel
/// once held (HoldDrag), and with `barsMoveWindow` a plain drag moves the
/// window; in the
/// bar under the traffic lights, the room left for them drags the window.
struct GroupHeader: View {
    let group: DockGroup
    var leading: CGFloat = 0
    var movesWindow = false
    @Environment(DockController.self) private var controller
    @Environment(\.dockContent) private var content
    @Environment(\.dockOptions) private var options

    var body: some View {
        let single = group.panels.count == 1
        HStack(spacing: 2) {
            if single, let custom = content.header(group.selected) {
                custom
                    .padding(.leading, 8)
                    .contentShape(Rectangle())
                    // Lowest priority: the header's own buttons and chips win.
                    .modifier(HoldDrag(panel: group.selected))
                    .contextMenu { PanelMenu(panel: group.selected, tabbed: false) }
                    // Its full width before the empty bar beside it gets any.
                    .layoutPriority(1)
            } else {
                ForEach(group.panels, id: \.self) { panel in
                    TabItem(panel: panel, selected: panel == group.selected, single: single)
                }
            }
            BarSpace(panel: group.selected, tabbed: !single)
                .onGeometryChange(for: CGFloat.self) { $0.frame(in: .named(DockSpace.name)).minX } action: { controller.barSpaceStarts[group.id] = $0 }
            if let accessory = content.accessory(group.selected) {
                accessory
            }
        }
        .padding(.leading, 6)
        .padding(.trailing, 6)
        .frame(height: options.headerHeight)
        .padding(.leading, leading)
        .background(alignment: .leading) {
            if movesWindow && leading > 0 {
                WindowSpace().frame(width: leading)
            }
        }
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.primary.opacity(0.07)).frame(height: 1)
        }
        // Where a dragged panel joins this group as a tab (DockController.barTarget).
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .named(DockSpace.name)) } action: { controller.bars[group.id] = $0 }
        .onDisappear {
            controller.bars[group.id] = nil
            controller.barSpaceStarts[group.id] = nil
        }
    }
}

/// The empty part of a bar: moves its panel (or the window, HoldDrag).
private struct BarSpace: View {
    let panel: PanelID
    let tabbed: Bool

    var body: some View {
        Color.clear
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle())
            .contextMenu { PanelMenu(panel: panel, tabbed: tabbed) }
            .modifier(HoldDrag(panel: panel))
    }
}

/// Every drag that can pick a panel up: a press held for `hold` lifts the
/// panel under the pointer (so you can see it's got), then it follows the
/// pointer. A drag that starts moving before then moves the window instead,
/// on a bar (with `DockOptions.barsMoveWindow`, or a panel in a window of
/// its own), and does nothing elsewhere. Mike asked for the hold everywhere
/// (28 Oct): a panel picked up straight away by its background came loose
/// when he meant something else. Then for titles and tabs he wanted it
/// gone (`needsHold`), as dragging one is plainly meant to move the panel.
///
/// On a bar (`fromPress`) the drag starts from the press, not 5 points in,
/// so the hold can be timed and the panel lifts while you hold. That takes
/// the press, which a bar's empty part and a header's buttons survive, but a
/// tab's click and a panel's own taps don't, so elsewhere the drag starts 5
/// points in, as it did before, and a second gesture alongside only notes
/// when the press began: the panel comes loose if that was `hold` ago.
struct HoldDrag: ViewModifier {
    let panel: PanelID
    /// On a bar: a plain drag moves the window, when the options allow.
    var movesWindow = true
    /// Times the hold from the press (a bar), rather than 5 points in.
    var fromPress = true
    /// Over the view's own gestures (a tab's), for a drag 5 points in.
    var high = false
    /// Picks the panel up as soon as it moves, with no hold: a title or
    /// tab, which is plainly for moving the panel (Mike, 28 Oct, after
    /// trying the hold everywhere).
    var needsHold = true
    @Environment(DockController.self) private var controller
    @Environment(\.dockOptions) private var options
    @State private var press: Press?
    /// When the press began, for a drag that starts 5 points in.
    @State private var pressedAt: Date?

    static let hold: Duration = .milliseconds(350)

    private enum Mode { case waiting, window, nothing, panel }

    private struct Press {
        var mode: Mode
        let start: CGPoint
        var location: CGPoint
        /// Where the pointer and the window were at the press, in screen points.
        let mouse: CGPoint
        let origin: CGPoint?
        weak var window: NSWindow?
        var timer: Task<Void, Never>?
    }

    func body(content: Content) -> some View {
        if fromPress {
            content.gesture(
                DragGesture(minimumDistance: 0, coordinateSpace: .named(DockSpace.name))
                    .onChanged(changed)
                    .onEnded { _ in ended() }
            )
        } else {
            let drag = DragGesture(minimumDistance: 5, coordinateSpace: .named(DockSpace.name))
                .onChanged(movedOnce)
                .onEnded { _ in ended() }
            // A long press, not a drag from the press: that took a tab's
            // click. Its change comes on the press, each time.
            let timing = LongPressGesture(minimumDuration: 0.01, maximumDistance: .infinity)
                .onChanged { _ in pressedAt = Date() }
            let placed = Group {
                if high { content.highPriorityGesture(drag) } else { content.gesture(drag) }
            }
            // No timing without a hold: it would only take clicks.
            if needsHold { placed.simultaneousGesture(timing) } else { placed }
        }
    }

    /// A drag that starts 5 points in: the panel if the press was held
    /// first, otherwise the window (on a bar) or nothing.
    private func movedOnce(_ value: DragGesture.Value) {
        if press == nil {
            let held = !needsHold || pressedAt.map { Date().timeIntervalSince($0) >= Self.holdSeconds } ?? false
            let window = Self.windowUnderMouse()
            press = Press(mode: held ? .panel : dragsWindow ? .window : .nothing, start: value.startLocation, location: value.location,
                          mouse: NSEvent.mouseLocation, origin: window?.frame.origin, window: window)
        }
        changed(value)
    }

    private static var holdSeconds: Double {
        Double(hold.components.seconds) + Double(hold.components.attoseconds) / 1e18
    }

    /// Whether a plain drag here moves the window.
    private var dragsWindow: Bool {
        movesWindow && (options.barsMoveWindow || controller.layout.isFloating(panel))
    }

    /// The window the press landed in. Not the key window, which it isn't
    /// yet on a click that brings the app forward.
    private static func windowUnderMouse() -> NSWindow? {
        let number = NSWindow.windowNumber(at: NSEvent.mouseLocation, belowWindowWithWindowNumber: 0)
        return NSApp.window(withWindowNumber: number)
    }

    private func changed(_ value: DragGesture.Value) {
        guard var p = press else {
            let window = Self.windowUnderMouse()
            var p = Press(mode: .waiting, start: value.startLocation, location: value.location,
                          mouse: NSEvent.mouseLocation, origin: window?.frame.origin, window: window)
            p.timer = Task { @MainActor in
                try? await Task.sleep(for: Self.hold)
                guard !Task.isCancelled, let held = press, held.mode == .waiting else { return }
                press?.mode = .panel
                controller.dragChanged(panel: panel, start: held.start, location: held.location)
            }
            press = p
            return
        }
        p.location = value.location
        if p.mode == .waiting, hypot(value.translation.width, value.translation.height) >= 5 {
            p.timer?.cancel()
            p.mode = dragsWindow ? .window : .nothing
        }
        press = p
        switch p.mode {
        case .waiting, .nothing: break
        case .panel: controller.dragChanged(panel: panel, start: p.start, location: value.location)
        case .window:
            // Moved by hand: the window is kept from moving itself (WindowGrip).
            guard let origin = p.origin, let window = p.window else { return }
            let mouse = NSEvent.mouseLocation
            window.setFrameOrigin(CGPoint(x: origin.x + mouse.x - p.mouse.x, y: origin.y + mouse.y - p.mouse.y))
        }
    }

    private func ended() {
        press?.timer?.cancel()
        if press?.mode == .panel { controller.dragEnded() }
        // A double-click that didn't move zooms the window, as a title
        // bar's does (and WindowGrip's). The drag starts on the press, so
        // it has to be looked for here, on the second click's release.
        if dragsWindow, press?.mode == .waiting, let event = NSApp.currentEvent,
           event.type == .leftMouseUp, event.clickCount == 2 {
            press?.window?.performZoom(nil)
        }
        press = nil
    }
}

/// Where a title bar would be, in a window without one: moves the window,
/// and a double-click zooms it, as a title bar's would.
private struct WindowSpace: View {
    @Environment(\.dockOptions) private var options

    var body: some View {
        let space = Color.clear
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle())
        if options.cornerMovesWindow {
            WindowGrip()
        } else {
            space
        }
    }
}

private struct TabItem: View {
    let panel: PanelID
    let selected: Bool
    let single: Bool
    @Environment(DockController.self) private var controller
    @Environment(\.dockContent) private var content
    @Environment(\.dockOptions) private var options
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 6) {
            Group {
                if let image = content.image(panel) {
                    Image(nsImage: image)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: options.titleSize + 2, height: options.titleSize + 2)
                        .opacity(selected ? 1 : 0.7)
                } else if let symbol = content.symbol(panel) {
                    Image(systemName: symbol)
                        .font(.system(size: options.titleSize - 1, weight: .medium))
                        .foregroundStyle(selected ? DockTheme.secondary : DockTheme.faint)
                }
            }
            .overlay(alignment: .bottomTrailing) { content.badge(panel) }
            Text(content.title(panel))
                .font(.system(size: options.titleSize, weight: selected ? .semibold : .regular))
                .foregroundStyle(selected ? DockTheme.headerText : DockTheme.secondary)
                .lineLimit(1)
                // A file's name keeps its extension when it has to shorten.
                .truncationMode(.middle)
                .modifier(TitleTip(text: content.titleTip(panel), options: options))
            if content.closable(panel) {
                Button {
                    controller.close(panel)
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: options.titleSize - 3, weight: .bold))
                        .foregroundStyle(DockTheme.secondary)
                        .frame(width: 16, height: 16)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .opacity(hovering || (selected && !single) ? 1 : 0)
                .dockTip("Close \(content.title(panel))", options)
            }
        }
        .padding(.leading, 8)
        .padding(.trailing, content.closable(panel) ? 4 : 8)
        .frame(height: max(options.headerHeight - 8, 20))
        .background(
            RoundedRectangle(cornerRadius: 6)
                .fill(Color.primary.opacity(selected && !single ? 0.07 : (hovering ? 0.04 : 0)))
        )
        // Full titles: the empty bar beside them gives way first. A panel
        // on its own still shortens a title (a long file name in the
        // pane) that wouldn't fit beside its buttons: the bar grew past
        // the panel and took the content with it, clipped at the edge.
        .fixedSize(horizontal: !single, vertical: false)
        .layoutPriority(single ? 1 : 0)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .onTapGesture { controller.select(panel) }
        // Middle-click closes a tab, as in a browser.
        .overlay {
            if content.closable(panel) { MiddleClick { controller.close(panel) } }
        }
        .contextMenu { PanelMenu(panel: panel, tabbed: !single) }
        // Mike couldn't tell a title could be dragged at all (5 Nov). The
        // close button's own tip wins over this one while over it.
        .dockTip(moveTip, options)
        // A tab (one that isn't showing too) is pulled out straight away,
        // with no hold. Its drag starts 5 points in, so a click still
        // selects the tab and reaches its close button: a drag from the
        // press swallowed both (28 Oct).
        .modifier(HoldDrag(panel: panel, fromPress: false, high: true, needsHold: false))
    }

    /// No hold to mention: a tab moves as soon as it's dragged (HoldDrag).
    private var moveTip: String {
        if controller.layout.isFloating(panel) { return "Drag to move, or onto the main window to dock it" }
        if controller.layout.canFloat(panel) { return "Drag to move, or out of the window to open it on its own" }
        return "Drag to move"
    }
}

/// Catches a middle-click over a view. SwiftUI has no gesture for the
/// middle button, so this watches the app's middle-button events and acts
/// on those that land on it with its window the topmost one there. It never
/// takes part in hit-testing, so clicks, drags and hovers reach the SwiftUI
/// view underneath as before.
private struct MiddleClick: NSViewRepresentable {
    let action: () -> Void

    func makeNSView(context: Context) -> ClickView { ClickView() }
    func updateNSView(_ view: ClickView, context: Context) { view.action = action }

    final class ClickView: NSView {
        var action: (() -> Void)?
        private var monitor: Any?
        private var pressed = false

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            // Off when the tab leaves its window, which it does before it goes.
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.otherMouseDown, .otherMouseUp]) { [weak self] event in
                guard let self, event.buttonNumber == 2 else { return event }
                let over = self.contains(event)
                if event.type == .otherMouseDown {
                    self.pressed = over
                    return over ? nil : event
                }
                // On release, and only over the tab, like a browser: pressing
                // then sliding off lets you change your mind.
                guard self.pressed else { return event }
                self.pressed = false
                if over { self.action?() }
                return nil
            }
        }

        private func contains(_ event: NSEvent) -> Bool {
            guard let window, !isHiddenOrHasHiddenAncestor else { return false }
            // Events from the window server carry their window; one with none
            // (posted by a test) is already in screen points.
            let screen = event.window.map { $0.convertPoint(toScreen: event.locationInWindow) } ?? event.locationInWindow
            guard NSWindow.windowNumber(at: screen, belowWindowWithWindowNumber: 0) == window.windowNumber else { return false }
            return bounds.contains(convert(window.convertPoint(fromScreen: screen), from: nil))
        }
    }
}

/// Right-click on a title, a tab, the strip of a panel without a title, or
/// a panel's background.
struct PanelMenu: View {
    let panel: PanelID
    let tabbed: Bool
    @Environment(DockController.self) private var controller
    @Environment(\.dockContent) private var content

    var body: some View {
        Group {
            if controller.layout.hiddenTitles.contains(panel) {
                Button("Show Title", systemImage: "eye") { controller.setTitleHidden(panel, false) }
            } else {
                // With tabs the bar has to stay, so say when it'll take effect.
                Button(tabbed ? "Hide Title When on Its Own" : "Hide Title", systemImage: "eye.slash") { controller.setTitleHidden(panel, true) }
            }
            if controller.layout.isFloating(panel) {
                Button("Put Back in Main Window", systemImage: "rectangle.portrait.and.arrow.forward") { controller.putBack(panel) }
            } else if controller.layout.canFloat(panel) {
                Button("Open in New Window", systemImage: "macwindow.badge.plus") { controller.openInWindow(panel) }
            }
            if content.closable(panel) {
                Divider()
                Button("Close \(content.title(panel))", systemImage: "xmark") { controller.close(panel) }
            }
        }
        // macOS hides a SwiftUI menu's icons without this on the items.
        .labelStyle(.titleAndIcon)
    }
}

/// In place of the title bar for a panel that hides its title: a strip
/// along the top that shows a grab handle on hover, to drag the panel by and
/// right-click for the title back.
private struct HiddenTitleStrip: View {
    let panel: PanelID
    @Environment(DockController.self) private var controller
    @Environment(\.dockOptions) private var options
    @State private var hovering = false

    var body: some View {
        ZStack {
            Capsule()
                .fill(Color.primary.opacity(hovering ? 0.28 : 0))
                .frame(width: 34, height: 4)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 10)
        .contentShape(Rectangle())
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.15)) { hovering = inside }
            if inside { NSCursor.openHand.push() } else { NSCursor.pop() }
        }
        .modifier(HoldDrag(panel: panel, movesWindow: false, fromPress: false))
        .contextMenu { PanelMenu(panel: panel, tabbed: false) }
        .dockTip("Press and hold, then drag to move this panel. Right-click to show its title.", options)
    }
}

/// The bar of a title-less panel under the traffic lights: empty, moving
/// the window (or the panel, with a title bar), with the grab handle on
/// hover to move the panel by. In Threadbear that's the sidebar's bar, the
/// one place left to grab the window by.
private struct CornerBar: View {
    let panel: PanelID
    @Environment(\.dockOptions) private var options

    var body: some View {
        HStack(spacing: 0) {
            if options.cornerMovesWindow {
                WindowSpace()
            } else {
                Color.clear.frame(width: options.trafficLights)
                BarSpace(panel: panel, tabbed: false)
            }
            HiddenTitleStrip(panel: panel)
                .frame(width: 60)
            Color.clear.frame(width: 12)
        }
        .frame(height: options.headerHeight)
    }
}

extension View {
    /// Keeps a region of a panel from picking the panel up when dragged,
    /// for content with no drag of its own (a canvas, a read-only list)
    /// where moving the panel would surprise. The header still moves it.
    public func dockDragDisabled() -> some View {
        // A drag that does nothing, nearer the pointer than the dock's.
        gesture(DragGesture(minimumDistance: 0).onChanged { _ in })
    }
}

/// The one place that moves a window whose panels' bars are its top. The
/// window itself is kept from moving (DockAnchor): with a hidden title bar
/// AppKit still treats the top 28 points as one and moved the window on a
/// drag of a panel's bar before the panel's own drag could start, the
/// SwiftUI gestures there notwithstanding (Mike, 28 Oct). So this lets it
/// move just for its own drag. Public for anything the app draws beside
/// the dock under the traffic lights (Threadbear's activity bar).
public struct WindowGrip: NSViewRepresentable {
    public init() {}
    public func makeNSView(context: Context) -> NSView { GripView() }
    public func updateNSView(_ view: NSView, context: Context) {}

    private final class GripView: NSView {
        override var mouseDownCanMoveWindow: Bool { false }

        override func mouseDown(with event: NSEvent) {
            guard let window else { return }
            if event.clickCount == 2 {
                window.performZoom(nil)
                return
            }
            window.isMovable = true
            // Runs the drag to mouse-up, then locks the window again.
            window.performDrag(with: event)
            window.isMovable = false
        }
    }
}

import AppKit
import Observation
import SwiftUI

/// What the compass in the middle of a group offers.
public enum CompassItem: Hashable, Sendable {
    case tab
    case split(DockEdge)
    case swap
    /// Out of the dock into a window of its own.
    case window

    var label: String {
        switch self {
        case .tab: "Add as a tab"
        case .swap: "Swap places"
        case .window: "Open in a new window"
        case .split(.left): "Dock on the left"
        case .split(.right): "Dock on the right"
        case .split(.top): "Dock above"
        case .split(.bottom): "Dock below"
        }
    }
}

/// Where the pointer is pointing while a panel is being dragged.
struct DropTarget: Equatable {
    /// Nil over the compass's gaps or a drop that would go nowhere.
    var drop: DockDrop?
    /// The space the panel would take, in the dock's coordinates.
    var preview: CGRect
    /// The compass, while the pointer is over the middle of a group.
    var compass: CompassState?
    /// Set when letting go would open the panel in a window of its own:
    /// that window's frame on screen. `drop` is nil then.
    var float: CGRect?
    /// Over a group's bar: `preview` is the new tab's place in the bar,
    /// drawn as a tab, and the panel lands in `land` (the whole group).
    var inBar = false
    var land: CGRect?
}

struct CompassState: Equatable {
    var group: String
    var center: CGPoint
    var hovered: CompassItem?
    /// Items that would do nothing here (the panel's own group), drawn dimmed.
    var disabled: Set<CompassItem>
}

struct DragState: Equatable {
    var panel: PanelID
    var sourceGroup: String
    /// The group's frame when the drag started.
    var sourceFrame: CGRect
    /// Where in the panel it was picked up, 0...1 each way, so that point
    /// stays under the pointer as it shrinks.
    var grab: CGPoint
    var location: CGPoint
    var lifted = false
    var target: DropTarget?
    /// A panel in a window of its own, dragged by its title: the window
    /// itself moves with the pointer, so there's no ghost, and it can be
    /// dropped back into the dock. The window's frame when it started.
    var window: CGRect?
}

/// The ghost flying to where the panel landed (or back home), after the drop.
struct Landing: Equatable {
    var panel: PanelID
    var sourceSize: CGSize
    var to: CGRect
    var done = false
}

/// The layout and everything about a drag in progress. One per dock.
@MainActor
@Observable
public final class DockController {
    public var layout: DockLayout {
        didSet { if layout != oldValue { onChange?(layout) } }
    }
    /// Called when the layout changes, to save it.
    @ObservationIgnored public var onChange: ((DockLayout) -> Void)?

    var drag: DragState?
    var landing: Landing?
    /// Each group's frame in the dock's coordinates, from the views.
    @ObservationIgnored var frames: [String: CGRect] = [:]
    /// Each group's bar, while it shows one, and where the empty part of it
    /// starts (after the title or tabs), for dropping a panel in as a tab.
    @ObservationIgnored var bars: [String: CGRect] = [:]
    @ObservationIgnored var barSpaceStarts: [String: CGFloat] = [:]
    @ObservationIgnored var bounds: CGRect = .zero
    /// Set by Escape until the gesture that was cancelled ends.
    @ObservationIgnored private var cancelled = false
    @ObservationIgnored private var escapeMonitor: Any?
    /// A view filling the dock, for turning its coordinates into the screen's.
    @ObservationIgnored weak var anchor: NSView?
    /// The panels in windows of their own, and the outline of one about to be.
    @ObservationIgnored lazy var windows = DockWindows(controller: self)

    // Feel. Tuned by eye in the demo.
    /// How far in from a group's side, as a share of its size, counts as that side.
    static let edgeBand: CGFloat = 0.28
    /// Within this of the window's side, the drop spans the whole side.
    static let windowBand: CGFloat = 22
    /// The ghost's width once lifted, within limits.
    static let ghostWidth: CGFloat = 240
    /// A new window takes the panel's size within these.
    static let windowMin = CGSize(width: 320, height: 260)
    static let windowMax = CGSize(width: 760, height: 680)
    static let lift = Animation.spring(response: 0.32, dampingFraction: 0.72)
    static let settle = Animation.spring(response: 0.34, dampingFraction: 0.86)
    static let track = Animation.snappy(duration: 0.16)

    public init(layout: DockLayout) {
        self.layout = layout
    }

    // MARK: Dragging

    /// From a drag gesture on a panel's body or its tab. Locations are in the
    /// dock's coordinate space.
    public func dragChanged(panel: PanelID, start: CGPoint, location: CGPoint) {
        // Escape put it back; the gesture keeps reporting until the button's up.
        if cancelled { return }
        if layout.isFloating(panel) {
            windowDragChanged(panel: panel)
            return
        }
        if drag == nil {
            guard landing == nil, let group = layout.group(containing: panel), let frame = frames[group.id] else { return }
            // A tab that isn't showing comes to the front as it's picked up.
            if group.selected != panel { layout.select(panel) }
            drag = DragState(
                panel: panel,
                sourceGroup: group.id,
                sourceFrame: frame,
                grab: CGPoint(x: (start.x - frame.minX) / max(frame.width, 1), y: (start.y - frame.minY) / max(frame.height, 1)),
                location: location
            )
            NSCursor.closedHand.push()
            watchForEscape()
            if ProcessInfo.processInfo.environment["BEARDOCK_LOG"] != nil { print("lift \(panel) from \(start)") }
            withAnimation(Self.lift) { drag?.lifted = true }
        }
        drag?.location = location
        var target = target(for: panel, at: location)
        // Outside the window altogether, or on the compass's New Window: a
        // window of its own, there.
        let wantsWindow = target?.compass?.hovered == .window || (target == nil && isOutsideWindow(location))
        if wantsWindow, let d = drag, layout.canFloat(panel) {
            target = DropTarget(drop: nil, preview: .zero, compass: target?.compass, float: floatFrame(for: d, at: location))
        }
        if target != drag?.target {
            withAnimation(Self.track) { drag?.target = target }
        }
        windows.showOutline(panel: panel, frame: target?.float)
    }

    /// A panel in a window of its own, dragged by its title: the window
    /// follows the pointer, and over the dock the drop targets show as for
    /// any other panel.
    private func windowDragChanged(panel: PanelID) {
        guard let window = windows.window(for: panel) else { return }
        let mouse = NSEvent.mouseLocation
        if drag == nil {
            guard landing == nil else { return }
            let f = window.frame
            drag = DragState(
                panel: panel,
                sourceGroup: "",
                sourceFrame: .zero,
                // Kept as an offset from the window's corner, in points.
                grab: CGPoint(x: mouse.x - f.minX, y: mouse.y - f.minY),
                location: .zero,
                lifted: true,
                window: f
            )
            NSCursor.closedHand.push()
            watchForEscape()
        }
        guard let d = drag else { return }
        window.setFrameOrigin(CGPoint(x: mouse.x - d.grab.x, y: mouse.y - d.grab.y))
        let location = dockPoint(screen: mouse)
        if let location { drag?.location = location }
        let target = location.flatMap { self.target(for: panel, at: $0) }
        if target != drag?.target {
            withAnimation(Self.track) { drag?.target = target }
        }
        // See-through over a place to dock, so the targets under it show.
        window.alphaValue = target?.drop != nil || target?.compass != nil ? 0.5 : 1
    }

    public func dragEnded() {
        if cancelled {
            cancelled = false
            return
        }
        guard let d = drag else { return }
        if d.window != nil {
            finishWindowDrag(d, cancelled: false)
        } else if let frame = d.target?.float {
            NSCursor.pop()
            stopWatchingForEscape()
            windows.showOutline(panel: d.panel, frame: nil)
            windows.focusNext = d.panel
            settle {
                drag = nil
                let home = home(of: d.panel)
                layout.float(d.panel, frame: frame, home: home.drop, share: home.share)
            }
        } else {
            finish(d, drop: d.target?.drop)
        }
    }

    /// Escape, mid-drag: the panel flies home and the rest of the gesture is ignored.
    public func cancelDrag() {
        guard let d = drag else { return }
        cancelled = true
        if d.window != nil {
            finishWindowDrag(d, cancelled: true)
        } else {
            finish(d, drop: nil)
        }
    }

    /// Lets go of a window's panel: into the dock where it's pointing, else
    /// the window stays where it was dragged (or goes back, after Escape).
    private func finishWindowDrag(_ d: DragState, cancelled: Bool) {
        NSCursor.pop()
        stopWatchingForEscape()
        let window = windows.window(for: d.panel)
        window?.alphaValue = 1
        if cancelled, let from = d.window {
            window?.setFrame(from, display: true, animate: true)
        }
        settle {
            drag = nil
            if !cancelled, let drop = d.target?.drop {
                layout.move(d.panel, drop)
            } else if let frame = window?.frame {
                layout.float(d.panel, frame: frame)
            }
        }
    }

    private func finish(_ d: DragState, drop: DockDrop?) {
        NSCursor.pop()
        stopWatchingForEscape()
        windows.showOutline(panel: d.panel, frame: nil)
        let size = d.sourceFrame.size
        if let drop, let to = d.target?.land ?? d.target?.preview {
            landing = Landing(panel: d.panel, sourceSize: size, to: to)
            settle {
                drag = nil
                layout.move(d.panel, drop)
                landing?.done = true
            }
        } else {
            // Nowhere to go, or cancelled: fly home.
            landing = Landing(panel: d.panel, sourceSize: size, to: frames[d.sourceGroup] ?? d.sourceFrame)
            settle {
                drag = nil
                landing?.done = true
            }
        }
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            withAnimation(.easeOut(duration: 0.12)) { self.landing = nil }
        }
    }

    /// A key monitor only while a drag's in progress, so Escape reaches the
    /// dock whatever has focus, and is swallowed rather than also closing a
    /// sheet or clearing a field.
    private func watchForEscape() {
        guard escapeMonitor == nil else { return }
        escapeMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard event.keyCode == 53, let self else { return event }
            MainActor.assumeIsolated { self.cancelDrag() }
            return nil
        }
    }

    private func stopWatchingForEscape() {
        if let m = escapeMonitor { NSEvent.removeMonitor(m) }
        escapeMonitor = nil
    }

    /// A panel's group's frame in the dock, for tests and the demo's
    /// frozen drags.
    public func frame(of panel: PanelID) -> CGRect? {
        layout.group(containing: panel).flatMap { frames[$0.id] }
    }

    /// The panel showing at `point` (in `window`'s coordinates): the front
    /// panel of the group there, or the panel a window of its own holds.
    /// Threadbear's activity log says where a click was with it.
    public func panel(at point: CGPoint, in window: NSWindow) -> PanelID? {
        if let own = window as? DockPanelWindow { return own.panel }
        guard let anchor, anchor.window === window else { return nil }
        let p = anchor.convert(point, from: nil)
        return layout.groups.first { frames[$0.id]?.contains(p) == true }?.selected
    }

    // MARK: Windows of their own

    /// Right-click, Open in New Window: the panel in a window of its own,
    /// just down and to the right of where it was.
    public func openInWindow(_ panel: PanelID) {
        guard layout.canFloat(panel), let frame = frame(of: panel) else { return }
        let size = Self.windowSize(for: frame.size)
        let at = CGRect(x: frame.minX + 30, y: frame.minY + 30, width: size.width, height: size.height)
        guard let screen = screenRect(at) else { return }
        windows.focusNext = panel
        let home = home(of: panel)
        settle { layout.float(panel, frame: screen, home: home.drop, share: home.share) }
    }

    /// A panel that isn't in the layout, straight into a window of its own
    /// in the middle of the dock's window, at `size` within the usual
    /// limits. Put Back docks it down the right. One that's in the layout
    /// already comes to the front instead. (Threadbear's show_panel, when an
    /// agent asks for a window.)
    public func openInNewWindow(_ panel: PanelID, size: CGSize = CGSize(width: 560, height: 520)) {
        guard !layout.contains(panel) else { return show(panel) }
        let s = Self.windowSize(for: size)
        let rect = CGRect(x: bounds.midX - s.width / 2, y: bounds.midY - s.height / 2, width: s.width, height: s.height)
        // Not on screen yet (no window to measure from): docked will do.
        guard bounds.width > 0, let screen = screenRect(rect) else { return open(panel, .window(.right)) }
        var next = layout
        next.add(panel, .window(.right))
        next.float(panel, frame: screen, home: .window(.right))
        windows.focusNext = panel
        settle { layout = next }
    }

    /// From a window of its own back into the dock, where it came from if
    /// that's still there.
    public func putBack(_ panel: PanelID) {
        guard let float = layout.floating.first(where: { $0.panel == panel }) else { return }
        let drop = float.home.flatMap { layout.canDrop(panel, $0) ? $0 : nil } ?? .window(.right)
        settle { layout.move(panel, drop) }
    }

    /// Where Put Back docks a panel: as a tab of the group it's leaving if
    /// that has others, else along the side of the window it's nearest, at
    /// the share of the window it has now.
    func home(of panel: PanelID) -> (drop: DockDrop?, share: Double?) {
        guard let group = layout.group(containing: panel) else { return (nil, nil) }
        if group.panels.count > 1 { return (.tab(group: group.id), nil) }
        guard let f = frames[group.id], bounds.width > 0, bounds.height > 0 else { return (nil, nil) }
        let sides: [(DockEdge, CGFloat)] = [
            (.left, f.midX - bounds.minX), (.right, bounds.maxX - f.midX),
            (.top, f.midY - bounds.minY), (.bottom, bounds.maxY - f.midY),
        ]
        let edge = sides.min { $0.1 < $1.1 }!.0
        let share = edge.axis == .horizontal ? f.width / bounds.width : f.height / bounds.height
        return (.window(edge), Double(share))
    }

    static func windowSize(for size: CGSize) -> CGSize {
        CGSize(width: min(max(size.width, windowMin.width), windowMax.width),
               height: min(max(size.height, windowMin.height), windowMax.height))
    }

    /// The window a dragged panel would open as, on screen: its own size
    /// (within limits), with the spot it was picked up by under the pointer.
    func floatFrame(for d: DragState, at p: CGPoint) -> CGRect? {
        let size = Self.windowSize(for: d.sourceFrame.size)
        let rect = CGRect(x: p.x - d.grab.x * size.width, y: p.y - d.grab.y * size.height, width: size.width, height: size.height)
        return screenRect(rect)
    }

    /// A rect in the dock's coordinates, on screen (origin bottom-left).
    func screenRect(_ rect: CGRect) -> CGRect? {
        guard let anchor, let window = anchor.window else { return nil }
        return window.convertToScreen(anchor.convert(rect, to: nil))
    }

    /// A point on screen in the dock's coordinates.
    func dockPoint(screen p: CGPoint) -> CGPoint? {
        guard let anchor, let window = anchor.window else { return nil }
        return anchor.convert(window.convertPoint(fromScreen: p), from: nil)
    }

    /// Past the edge of the window the dock's in, not just the dock (the
    /// app may have more beside it, like Threadbear's rail).
    func isOutsideWindow(_ p: CGPoint) -> Bool {
        guard let anchor, let content = anchor.window?.contentView else { return !bounds.contains(p) }
        return !anchor.convert(content.bounds, from: content).contains(p)
    }

    // MARK: Other changes

    /// Where each panel's group was drawn when the layout last changed with
    /// its glide, by the panel showing in it, for scripts: `panelFrames`
    /// says where they went.
    @ObservationIgnored public private(set) var motionFrom: [PanelID: CGRect]?

    /// Each group's frame in the dock, by the panel showing in it.
    public var panelFrames: [PanelID: CGRect] {
        Dictionary(layout.groups.compactMap { g in frames[g.id].map { (g.selected, $0) } }, uniquingKeysWith: { a, _ in a })
    }

    /// A layout change, with the groups gliding to their new places
    /// (`matchedGeometryEffect` in GroupView), noting where they were first.
    private func settle(_ change: () -> Void) {
        motionFrom = panelFrames
        withAnimation(Self.settle, change)
    }

    public func close(_ panel: PanelID) {
        settle { layout.close(panel) }
    }

    public func setTitleHidden(_ panel: PanelID, _ hidden: Bool) {
        withAnimation(.snappy(duration: 0.22)) { layout.setTitleHidden(panel, hidden) }
    }

    public func select(_ panel: PanelID) {
        layout.select(panel)
    }

    /// In the layout and the tab showing in its group, or in a window of its own.
    public func isShowing(_ panel: PanelID) -> Bool {
        layout.isFloating(panel) || layout.group(containing: panel)?.selected == panel
    }

    /// Brings a panel up: its tab if it's in the layout, else opened as a
    /// tab beside the first of `near` that's open, else at `fallback`.
    public func show(_ panel: PanelID, near: [PanelID] = [], fallback: DockDrop = .window(.right)) {
        if layout.isFloating(panel) {
            windows.window(for: panel)?.makeKeyAndOrderFront(nil)
            return
        }
        if let group = layout.group(containing: panel) {
            if group.selected != panel { layout.select(panel) }
            return
        }
        let drop = near.lazy.compactMap { self.layout.group(containing: $0) }.first.map { DockDrop.tab(group: $0.id) } ?? fallback
        open(panel, drop)
    }

    /// A rail button, VS Code's way: closes a panel that's showing, brings
    /// up one that's open behind another tab, opens one that isn't.
    public func toggle(_ panel: PanelID, near: [PanelID] = [], fallback: DockDrop = .window(.right)) {
        if isShowing(panel) { close(panel) } else { show(panel, near: near, fallback: fallback) }
    }

    /// Opens a panel that isn't in the layout, along the right of the window.
    public func open(_ panel: PanelID, _ drop: DockDrop = .window(.right)) {
        settle { layout.add(panel, drop) }
    }

    public func setLayout(_ layout: DockLayout) {
        settle { self.layout = layout }
    }

    // MARK: Hit testing

    func target(for panel: PanelID, at p: CGPoint) -> DropTarget? {
        if isOutsideWindow(p) { return nil }
        // A group's bar: join it as a tab, shown as a tab in the bar (Mike,
        // 28 Oct). Over the window's top band too, which runs along the top
        // row's bars, except its outermost few points, so docking along the
        // whole top of the window is still there.
        if p.y - bounds.minY >= Self.windowTopOverBars, let target = barTarget(for: panel, at: p) {
            return target
        }
        // The window's own sides first: a thin strip all along each.
        if bounds.width > 0, let edge = windowEdge(at: p), layout.canDrop(panel, .window(edge)) {
            return DropTarget(drop: .window(edge), preview: Self.slice(bounds, edge, 0.25))
        }
        guard let group = layout.groups.first(where: { frames[$0.id]?.contains(p) == true }),
              let frame = frames[group.id] else { return nil }

        let center = CGPoint(x: frame.midX, y: frame.midY)
        let items = Self.compassItems(center: center)
        let compassArea = items.reduce(CGRect.null) { $0.union($1.1) }.insetBy(dx: -14, dy: -14)

        // The sides of the group, unless the pointer is on the compass.
        if !compassArea.contains(p) {
            let fx = (p.x - frame.minX) / frame.width, fy = (p.y - frame.minY) / frame.height
            let sides: [(DockEdge, CGFloat)] = [(.left, fx), (.right, 1 - fx), (.top, fy), (.bottom, 1 - fy)]
            let (edge, distance) = sides.min { $0.1 < $1.1 }!
            if distance < Self.edgeBand {
                let drop = DockDrop.edge(group: group.id, edge)
                return DropTarget(drop: layout.canDrop(panel, drop) ? drop : nil, preview: Self.slice(frame, edge, 0.5))
            }
        }

        // The middle: the compass, and whatever it's on (a tab by default).
        let disabled = Set(items.map(\.0).filter { item in
            if item == .window { return !layout.canFloat(panel) }
            return !layout.canDrop(panel, Self.drop(for: item, group: group.id))
        })
        let hovered = items.first { $0.1.contains(p) }?.0
        let item = hovered ?? .tab
        // New window isn't a place in the dock: dragChanged works out its frame.
        let drop = disabled.contains(item) || item == .window ? nil : Self.drop(for: item, group: group.id)
        let preview: CGRect = switch item {
        case .split(let edge): Self.slice(frame, edge, 0.5)
        case .tab, .swap, .window: frame
        }
        return DropTarget(
            drop: drop,
            preview: preview,
            compass: CompassState(group: group.id, center: center, hovered: hovered, disabled: disabled)
        )
    }

    /// Over a bar, the window's top band is only this deep.
    static let windowTopOverBars: CGFloat = 6
    /// The width of the tab drawn in a bar for a drop there.
    static let barTabWidth: CGFloat = 120

    private func barTarget(for panel: PanelID, at p: CGPoint) -> DropTarget? {
        guard let group = layout.groups.first(where: { layout.showsHeader($0) && bars[$0.id]?.contains(p) == true }),
              let bar = bars[group.id], let frame = frames[group.id] else { return nil }
        let drop = DockDrop.tab(group: group.id)
        // After the titles, or as far along as there's room for.
        let start = barSpaceStarts[group.id] ?? bar.minX
        let width = min(Self.barTabWidth, max(bar.width - 16, 40))
        let x = max(bar.minX + 4, min(start + 4, bar.maxX - width - 4))
        let tab = CGRect(x: x, y: bar.minY + 4, width: width, height: max(bar.height - 8, 12))
        return DropTarget(drop: layout.canDrop(panel, drop) ? drop : nil, preview: tab, inBar: true, land: frame)
    }

    private func windowEdge(at p: CGPoint) -> DockEdge? {
        let b = Self.windowBand
        if p.x - bounds.minX < b { return .left }
        if bounds.maxX - p.x < b { return .right }
        if p.y - bounds.minY < b { return .top }
        if bounds.maxY - p.y < b { return .bottom }
        return nil
    }

    static func drop(for item: CompassItem, group: String) -> DockDrop {
        switch item {
        case .tab, .window: .tab(group: group)
        case .swap: .swap(group: group)
        case .split(let edge): .edge(group: group, edge)
        }
    }

    /// The compass's buttons around a group's centre: a cross of five, and
    /// Swap and New Window side by side under it.
    static func compassItems(center c: CGPoint) -> [(CompassItem, CGRect)] {
        let s = Compass.button, step = Compass.button + Compass.gap
        func at(_ dx: CGFloat, _ dy: CGFloat) -> CGRect {
            CGRect(x: c.x + dx * step - s / 2, y: c.y + dy * step - s / 2, width: s, height: s)
        }
        let y = c.y + 2 * step - s / 2 + 6
        let swap = CGRect(x: c.x - Compass.swapWidth - Compass.gap / 2, y: y, width: Compass.swapWidth, height: Compass.swapHeight)
        let window = CGRect(x: c.x + Compass.gap / 2, y: y, width: Compass.swapWidth, height: Compass.swapHeight)
        return [
            (.tab, at(0, 0)),
            (.split(.top), at(0, -1)),
            (.split(.bottom), at(0, 1)),
            (.split(.left), at(-1, 0)),
            (.split(.right), at(1, 0)),
            (.swap, swap),
            (.window, window),
        ]
    }

    /// The share of a rect along one side.
    static func slice(_ r: CGRect, _ edge: DockEdge, _ share: CGFloat) -> CGRect {
        switch edge {
        case .left: CGRect(x: r.minX, y: r.minY, width: r.width * share, height: r.height)
        case .right: CGRect(x: r.maxX - r.width * share, y: r.minY, width: r.width * share, height: r.height)
        case .top: CGRect(x: r.minX, y: r.minY, width: r.width, height: r.height * share)
        case .bottom: CGRect(x: r.minX, y: r.maxY - r.height * share, width: r.width, height: r.height * share)
        }
    }

    /// The ghost's rect while lifted: shrunk, with the grab point under the pointer.
    func ghostRect(for d: DragState) -> CGRect {
        let size = d.sourceFrame.size
        let scale = d.lifted ? min(0.85, max(0.3, Self.ghostWidth / max(size.width, 1))) : 1
        let w = size.width * scale, h = size.height * scale
        return CGRect(x: d.location.x - d.grab.x * w, y: d.location.y - d.grab.y * h, width: w, height: h)
    }
}

enum Compass {
    static let button: CGFloat = 36
    static let gap: CGFloat = 6
    static let swapWidth: CGFloat = 84
    static let swapHeight: CGFloat = 26
}

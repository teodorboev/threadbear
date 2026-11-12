import AppKit
import BearDock
import SwiftUI

// A playground for BearDock: Threadbear-ish mock panels to drag about until
// docking feels right. The layout is saved between launches; Reset puts it
// back to option 2 (threads | transcript | highlights over a panel).

@main
struct DockDemoApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        WindowGroup("BearDock") {
            DemoView()
                .frame(minWidth: 800, minHeight: 500)
                .preferredColorScheme(.dark)
        }
        .defaultSize(width: 1320, height: 820)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Run from `swift run`, it's a bare binary: make it a normal app with
        // a Dock icon and bring its window forward.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

enum Demo {
    static let all: [PanelID] = ["threads", "transcript", "highlights", "convex", "worktrees", "terminal", "files"]

    static func title(_ p: PanelID) -> String {
        switch p {
        case "threads": "Threads"
        case "transcript": "Transcript"
        case "highlights": "Highlights"
        case "convex": "Convex dashboard"
        case "worktrees": "Worktrees"
        case "terminal": "Terminal"
        case "files": "Files"
        default: p
        }
    }

    static func symbol(_ p: PanelID) -> String? {
        switch p {
        case "threads": "bubble.left.and.bubble.right"
        case "transcript": "text.alignleft"
        case "highlights": "pin"
        case "convex": "cylinder"
        case "worktrees": "arrow.triangle.branch"
        case "terminal": "terminal"
        case "files": "folder"
        default: nil
        }
    }

    static var starting: DockLayout {
        DockLayout(root: .split(DockSplit(id: "root", axis: .horizontal, children: [
            .group(DockGroup(id: "left", panels: ["threads"])),
            .group(DockGroup(id: "middle", panels: ["transcript"])),
            .split(DockSplit(id: "right", axis: .vertical, children: [
                .group(DockGroup(id: "right-top", panels: ["highlights", "worktrees", "terminal", "files"])),
                .group(DockGroup(id: "right-bottom", panels: ["convex"])),
            ], fractions: [0.55, 0.45])),
        ], fractions: [0.24, 0.5, 0.26])), hiddenTitles: ["threads"])
    }

    static let key = "layout"

    static func saved() -> DockLayout? {
        guard ProcessInfo.processInfo.environment["BEARDOCK_FRESH"] == nil,
              let data = UserDefaults.standard.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(DockLayout.self, from: data)
    }

    static func save(_ layout: DockLayout) {
        // A fresh run (screenshots, tests) leaves the saved layout alone.
        guard ProcessInfo.processInfo.environment["BEARDOCK_FRESH"] == nil else { return }
        UserDefaults.standard.set(try? JSONEncoder().encode(layout), forKey: key)
    }
}

struct DemoView: View {
    @State private var dock: DockController = {
        let c = DockController(layout: Demo.saved() ?? Demo.starting)
        c.onChange = { layout in
            if ProcessInfo.processInfo.environment["BEARDOCK_LOG"] != nil { print("layout \(layout.summary)") }
            Demo.save(layout)
        }
        return c
    }()

    private static var options: DockOptions {
        var o = DockOptions()
        // A panel's window of its own gets the demo's look too.
        o.windowContent = { AnyView($0.preferredColorScheme(.dark)) }
        // The app's bars move its window; this tries them the same way.
        o.barsMoveWindow = ProcessInfo.processInfo.environment["BEARDOCK_BARS_MOVE_WINDOW"] != nil
        return o
    }

    var body: some View {
        let closed = Demo.all.filter { !dock.layout.contains($0) }
        DockView(controller: dock, options: Self.options, title: Demo.title, symbol: Demo.symbol) { panel in
            MockPanel(panel: panel)
        }
        .toolbar {
            ToolbarItem {
                Menu {
                    ForEach(closed, id: \.self) { p in
                        Button(Demo.title(p)) { dock.open(p) }
                    }
                } label: {
                    Label("Open panel", systemImage: "plus.rectangle.on.rectangle")
                }
                .disabled(closed.isEmpty)
                .help(closed.isEmpty ? "Every panel is open" : "Open a closed panel")
            }
            ToolbarItem {
                Button {
                    dock.setLayout(Demo.starting)
                } label: {
                    Label("Reset layout", systemImage: "arrow.counterclockwise")
                }
                .help("Put the panels back where they started")
            }
        }
        .task { await FrozenDrag.run(dock) }
        .task { await ScriptedEvents.run() }
    }
}

/// For screenshots: `BEARDOCK_FREEZE=edge|compass|compass-left|window` holds
/// a drag of Highlights in mid-air, through the same path a real drag takes.
@MainActor
enum FrozenDrag {
    static func run(_ dock: DockController) async {
        guard var mode = ProcessInfo.processInfo.environment["BEARDOCK_FREEZE"] else { return }
        // "edge+drop" lets go at the end, to see where it lands.
        let drops = mode.hasSuffix("+drop")
        mode = mode.replacingOccurrences(of: "+drop", with: "")
        try? await Task.sleep(for: .milliseconds(600))
        guard let from = dock.frame(of: "highlights"), let mid = dock.frame(of: "transcript") else { return }
        let start = CGPoint(x: from.midX, y: from.minY + 120)
        let to: CGPoint = switch mode {
        case "edge": CGPoint(x: mid.minX + mid.width * 0.12, y: mid.midY)
        case "compass": CGPoint(x: mid.midX + 8, y: mid.midY + 6)
        case "compass-left": CGPoint(x: mid.midX - 42, y: mid.midY)
        case "window": CGPoint(x: 8, y: mid.midY)
        default: CGPoint(x: mid.midX, y: mid.minY + mid.height * 0.1)
        }
        // A few steps, so it lifts and then travels.
        for i in 1...12 {
            let t = CGFloat(i) / 12
            dock.dragChanged(panel: "highlights", start: start, location: CGPoint(x: start.x + (to.x - start.x) * t, y: start.y + (to.y - start.y) * t))
            try? await Task.sleep(for: .milliseconds(16))
        }
        if drops { dock.dragEnded() }
    }
}

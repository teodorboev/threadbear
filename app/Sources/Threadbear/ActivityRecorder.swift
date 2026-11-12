import AppKit
import BearDock
import ThreadbearCore

/// The activity log (docs/guide.md, "The activity log"): what's clicked,
/// opened, chosen and sent in the app, a line each (ActivityEvent), in
/// `<data folder>/activity/<day>.jsonl`, so an agent working on Threadbear
/// can see how it's really used (`tb activity`). Mike asked for it on 29 Oct.
///
/// Off unless it's switched on in Settings > General: once other people run
/// the app it mustn't look like it's watching them. Nothing sends it
/// anywhere, and files go after 30 days. It never has what's typed or the
/// words of a message: keys only with ⌘ or ⌃ (and Escape), and sends and
/// answers by count. A tooltip or menu title can name a thread or a folder
/// (a sidebar row's tip is its thread's title), so those can be in it.
///
/// What's clicked is named by the tooltip of the control under the pointer
/// (Tips), which every control has (AGENTS.md), so nothing else in the app
/// needs to say what it is.
@MainActor
final class ActivityRecorder {
    static let shared = ActivityRecorder()
    /// The Settings switch, in UserDefaults. Off unless it's turned on.
    static let enabledKey = "activityLog"

    static var isOn: Bool { !scripted && UserDefaults.standard.bool(forKey: enabledKey) }

    /// Perf and screenshot runs share the installed app's settings, and
    /// what they click isn't Mike.
    private static let scripted = CommandLine.arguments.contains("--perf-run") || CommandLine.arguments.contains("--screenshots")

    private weak var app: AppModel?
    private var writer: ActivityWriter?
    private var monitors: [Any] = []
    private var observers: [NSObjectProtocol] = []
    /// A shortcut waiting a moment to see whether it was a menu item's,
    /// which comes back as that item with its key, rather than twice.
    private var pendingKey: (name: String, event: NSEvent)?
    private var lastPlace: NavigationPlace?
    private var lastShowing: [String]?
    private var lastWindow: NSWindow?
    private var lastHover: (text: String, at: Date)?
    /// The running build, read at launch. By the time a handover's old app
    /// quits, its bundle's path already holds the new build, so reading it
    /// then named the wrong one (29 Oct).
    private var build: String?

    func install(app: AppModel) {
        guard writer == nil else { return }
        self.app = app
        writer = ActivityWriter(folder: ActivityFiles.folder(in: app.dataDir))
        let clicks = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
            self?.click(event)
            return event
        }
        let keys = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            self?.key(event)
            return event
        }
        monitors = [clicks, keys].compactMap { $0 }
        let center = NotificationCenter.default
        // Straight away, not queued: a shortcut's item has to find its key
        // still waiting (key()).
        observers.append(center.addObserver(forName: NSMenu.didSendActionNotification, object: nil, queue: nil) { [weak self] note in
            let item = note.userInfo?["MenuItem"] as? NSMenuItem
            MainActor.assumeIsolated { if let item { self?.menu(item) } }
        })
        observers.append(center.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.appState("active") }
        })
        observers.append(center.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.appState("inactive") }
        })
        observers.append(center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main) { [weak self] note in
            let window = note.object as? NSWindow
            MainActor.assumeIsolated { if let window { self?.windowBecameKey(window) } }
        })
        var launch = ActivityEvent(kind: .launch)
        launch.version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        build = BuildInfo.read(bundle: Bundle.main.bundleURL)?.short
        launch.build = build
        launch.showing = showing(app.dock.layout)
        lastShowing = launch.showing
        record(launch)
    }

    /// The last line, written before the app goes. With its build: in a
    /// handover the new app has launched before the old one quits.
    func quit() {
        var e = ActivityEvent(kind: .quit)
        e.build = build
        record(e)
        writer?.flush()
    }

    private func record(_ event: ActivityEvent) {
        guard Self.isOn, let writer, let line = try? event.line() else { return }
        writer.append(line, day: ActivityFiles.fileName(for: event.at))
    }

    // MARK: What happened

    func click(_ event: NSEvent) {
        guard Self.isOn, let window = event.window else { return }
        let point = event.locationInWindow
        var e = ActivityEvent(kind: .click)
        e.target = Tips.shared.text(at: point, in: window).flatMap { ActivityEvent.label($0) }
        if e.target == nil { e.view = Self.viewName(at: point, in: window) }
        // A transcript card's button has no tooltip: name the kind, so the
        // done card's Archive can be counted (the looseEnds check-in of
        // 11 Nov couldn't tell it from any other click on the transcript).
        if e.target == nil, let action = Self.transcriptAction(at: point, in: window) { e.target = "Card button: \(action)" }
        e.window = Self.name(of: window)
        let panel = app?.dock.panel(at: point, in: window)
        e.panel = panel.map(Self.panelName)
        e.thread = panel.flatMap(WindowDock.pinnedThread) ?? app?.selection
        switch event.type {
        case .rightMouseDown: e.button = "right"
        case .otherMouseDown: e.button = event.buttonNumber == 2 ? "middle" : "button \(event.buttonNumber + 1)"
        default: break
        }
        if event.clickCount > 1 { e.clicks = event.clickCount }
        let flags = event.modifierFlags
        let held = ActivityKeys.modifiers(command: flags.contains(.command), control: flags.contains(.control), option: flags.contains(.option), shift: flags.contains(.shift))
        if !held.isEmpty { e.modifiers = held }
        record(e)
    }

    private func key(_ event: NSEvent) {
        guard Self.isOn, !event.isARepeat else { return }
        let flags = event.modifierFlags
        guard let name = ActivityKeys.shortcut(keyCode: event.keyCode, characters: event.charactersIgnoringModifiers,
                                               command: flags.contains(.command), control: flags.contains(.control),
                                               option: flags.contains(.option), shift: flags.contains(.shift))
        else { return }
        flushKey()
        pendingKey = (name, event)
        // A menu's key equivalent is handled straight after this, in the
        // same dispatch, so by the next turn of the main queue menu() has
        // claimed it if it was one.
        DispatchQueue.main.async { [weak self] in self?.flushKey() }
    }

    private func flushKey() {
        guard let pending = pendingKey else { return }
        pendingKey = nil
        var e = ActivityEvent(kind: .key)
        e.target = pending.name
        if let window = pending.event.window { e.window = Self.name(of: window) }
        e.thread = app?.selection
        record(e)
    }

    private func menu(_ item: NSMenuItem) {
        guard Self.isOn else { return }
        var e = ActivityEvent(kind: .menu)
        let path = [item.parent?.title ?? item.menu?.title, item.title].compactMap { $0 }.filter { !$0.isEmpty }
        e.target = ActivityEvent.label(path.joined(separator: " › ")) ?? "untitled item"
        // A key still waiting was pressed in this same turn of the event
        // loop, so it's the one that chose this item.
        if let pending = pendingKey {
            e.key = pending.name
            pendingKey = nil
        }
        e.thread = app?.selection
        record(e)
    }

    /// A tooltip came up (Tips): the pointer stopped on something to find
    /// out what it is. The same one again within a few seconds is one look.
    func hover(_ text: String, in window: NSWindow) {
        guard Self.isOn, let label = ActivityEvent.label(text) else { return }
        if let last = lastHover, last.text == label, Date().timeIntervalSince(last.at) < 5 { return }
        lastHover = (label, Date())
        var e = ActivityEvent(kind: .hover)
        e.target = label
        e.window = Self.name(of: window)
        record(e)
    }

    /// Where the window is now (AppModel's navigation), once per change.
    func view(_ place: NavigationPlace) {
        guard Self.isOn, place != lastPlace else { return }
        lastPlace = place
        var e = ActivityEvent(kind: .view)
        e.mode = "\(place.mode)"
        e.thread = place.threadId
        e.subagent = place.subagent?.subagentId
        e.project = place.projectId
        e.page = place.projectsHome ? "projects" : place.librarianHome ? "librarian" : place.scheduledHome ? "scheduled" : nil
        record(e)
    }

    /// The dock's layout changed (WindowDock): recorded when what's showing
    /// did, not for every divider dragged.
    func panels(_ layout: DockLayout) {
        let now = showing(layout)
        guard Self.isOn, now != lastShowing else { return }
        lastShowing = now
        var e = ActivityEvent(kind: .panels)
        e.showing = now
        record(e)
    }

    func sent(threadId: String?, images: Int, answers: Int, edit: Bool = false) {
        guard Self.isOn else { return }
        var e = ActivityEvent(kind: .send)
        e.thread = threadId
        if threadId == nil { e.new = true }
        if edit { e.edit = true }
        if images > 0 { e.images = images }
        if answers > 0 { e.answers = answers }
        record(e)
    }

    func answered(threadId: String, decision: ApprovalDecision) {
        guard Self.isOn else { return }
        var e = ActivityEvent(kind: .approval)
        e.thread = threadId
        e.decision = decision.rawValue
        record(e)
    }

    private func appState(_ state: String) {
        guard Self.isOn else { return }
        var e = ActivityEvent(kind: .app)
        e.state = state
        record(e)
    }

    /// Only a change of window: the main one coming back with the app is
    /// already in "app active".
    private func windowBecameKey(_ window: NSWindow) {
        guard window !== lastWindow else { return }
        lastWindow = window
        guard Self.isOn else { return }
        var e = ActivityEvent(kind: .window)
        e.window = Self.name(of: window)
        if let panel = (window as? DockPanelWindow)?.panel { e.panel = Self.panelName(panel) }
        record(e)
    }

    // MARK: Names

    private func showing(_ layout: DockLayout) -> [String] {
        (layout.groups.map(\.selected) + layout.floating.map(\.panel)).map(Self.panelName)
    }

    static func panelName(_ panel: PanelID) -> String {
        panel.hasPrefix("threadbear.") ? String(panel.dropFirst("threadbear.".count)) : panel
    }

    static func name(of window: NSWindow) -> String {
        if window === MainWindowFrame.window { return "main" }
        if window === SettingsWindow.current { return "settings" }
        if window is DockPanelWindow { return "panel" }
        let type = String(describing: Swift.type(of: window))
        if type.contains("Popover") { return "popover" }
        if type.contains("Menu") { return "menu" }
        return ActivityEvent.label(window.title) ?? type
    }

    /// The kind of card button a click in the transcript's text landed on.
    static func transcriptAction(at point: NSPoint, in window: NSWindow) -> String? {
        guard let content = window.contentView else { return nil }
        var view = content.hitTest(content.superview?.convert(point, from: nil) ?? point)
        while let v = view {
            if let text = v as? TranscriptTextView { return text.activityAction(atWindow: point) }
            view = v.superview
        }
        return nil
    }

    /// For a click on something without a tooltip: the nearest of the
    /// app's own AppKit views under it (the transcript's text, the
    /// composer), or nil inside SwiftUI, whose views have no useful names.
    static func viewName(at point: NSPoint, in window: NSWindow) -> String? {
        guard let content = window.contentView else { return nil }
        var view = content.hitTest(content.superview?.convert(point, from: nil) ?? point)
        while let v = view {
            let name = NSStringFromClass(Swift.type(of: v))
            for module in ["Threadbear.", "BearDock."] where name.hasPrefix(module) {
                return String(name.dropFirst(module.count))
            }
            view = v.superview
        }
        return nil
    }
}

/// Appends the log's lines on a queue of its own, so a click never waits
/// on the disk. Each line is one write to a file opened for appending,
/// so two apps writing at once (a handover's old and new) never
/// interleave within a line.
final class ActivityWriter: @unchecked Sendable {
    private let folder: URL
    private let queue = DispatchQueue(label: "com.mikerosoft.threadbear.activity", qos: .utility)
    // On the queue only.
    private var day: String?
    private var handle: FileHandle?
    private var full = false

    init(folder: URL) {
        self.folder = folder
    }

    func append(_ line: Data, day: String) {
        queue.async { self.write(line + Data("\n".utf8), day: day) }
    }

    /// Waits for what's queued, for the last line before quitting.
    func flush() {
        queue.sync {}
    }

    private func write(_ data: Data, day: String) {
        if day != self.day { open(day) }
        guard let handle, !full else { return }
        if (try? handle.offset()) ?? 0 > ActivityFiles.maxDayBytes {
            full = true
            AppLog.write("activity", "\(day) is past \(ActivityFiles.maxDayBytes / 1_048_576) MB; nothing more is kept today")
            return
        }
        do {
            try handle.write(contentsOf: data)
        } catch {
            AppLog.write("activity", "couldn't write: \(error.localizedDescription)")
        }
    }

    private func open(_ day: String) {
        try? handle?.close()
        handle = nil
        full = false
        self.day = day
        let manager = FileManager.default
        do {
            try manager.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            // Time Machine leaves it out; it's only for working on the app.
            var url = folder
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try? url.setResourceValues(values)
            prune()
            let path = folder.appendingPathComponent(day).path
            let fd = Darwin.open(path, O_WRONLY | O_APPEND | O_CREAT, 0o600)
            guard fd >= 0 else {
                AppLog.write("activity", "couldn't open \(path): errno \(errno)")
                return
            }
            handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
            _ = try? handle?.seekToEnd()
        } catch {
            AppLog.write("activity", "couldn't make \(folder.path): \(error.localizedDescription)")
        }
    }

    private func prune() {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
        for name in ActivityFiles.expired(names, now: Date()) {
            try? FileManager.default.removeItem(at: folder.appendingPathComponent(name))
        }
    }
}

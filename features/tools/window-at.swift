import AppKit
import ApplicationServices

private func frame(_ window: [String: Any]) -> CGRect? {
    guard let b = window[kCGWindowBounds as String] as? [String: Double],
          let x = b["X"], let y = b["Y"], let w = b["Width"], let h = b["Height"] else { return nil }
    return CGRect(x: x, y: y, width: w, height: h)
}

// The injected hit-test keeps the fail-closed decision testable without a
// desktop, Accessibility permission or real input events.
func windowAt(_ windows: [[String: Any]], point: CGPoint, displays: [CGRect], dockPID: Int32?, notificationPIDs: Set<Int32> = [], hitPID: () -> Int32?) -> [String: Any]? {
    let covering = windows.filter {
        // Only the system process's sentinel is see-through. An ordinary
        // app can use the same title, and notification banners still block.
        if $0[kCGWindowName as String] as? String == "Notification Center",
           let pid = $0[kCGWindowOwnerPID as String] as? Int32,
           notificationPIDs.contains(pid) { return false }
        return ($0[kCGWindowAlpha as String] as? Double ?? 1) > 0
            && (frame($0)?.contains(point) ?? false)
    }
    guard let top = covering.first else { return nil }
    // The Dock can leave an almost transparent, display-sized layer-20 window
    // above every app. Bounds alone then refuse every drag (13 Nov). Only
    // accept the ordinary window directly beneath it when AX proves input at
    // this point reaches that same process. Mission Control, AX failures and
    // other overlays must keep blocking input.
    guard let dockPID,
          top[kCGWindowOwnerPID as String] as? Int32 == dockPID,
          top[kCGWindowOwnerName as String] as? String == "Dock",
          top[kCGWindowName as String] as? String == "Dock",
          top[kCGWindowLayer as String] as? Int == 20,
          let bounds = frame(top), displays.contains(bounds),
          // Moving the pointer to an edge can reveal an auto-hidden Dock
          // after this preflight. Never allow that through the exception.
          bounds.insetBy(dx: 24, dy: 24).contains(point),
          let beneath = covering.dropFirst().first,
          beneath[kCGWindowLayer as String] as? Int == 0,
          let pid = beneath[kCGWindowOwnerPID as String] as? Int32,
          hitPID() == pid else { return top }
    return beneath
}

#if !TESTING
let point = CGPoint(x: Double(CommandLine.arguments[1])!, y: Double(CommandLine.arguments[2])!)
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
var ids = [CGDirectDisplayID](repeating: 0, count: 32)
var count: UInt32 = 0
let displayError = CGGetActiveDisplayList(UInt32(ids.count), &ids, &count)
let displays = displayError == .success ? ids.prefix(Int(count)).map { CGDisplayBounds($0) } : []
let dockPID = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.dock")
    .first { $0.bundleURL?.path == "/System/Library/CoreServices/Dock.app" }?.processIdentifier
let notificationPIDs = Set(NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.notificationcenterui")
    .filter { $0.bundleURL?.path == "/System/Library/CoreServices/NotificationCenter.app" }
    .map { $0.processIdentifier })
let result = windowAt(windows, point: point, displays: displays, dockPID: dockPID, notificationPIDs: notificationPIDs) {
    guard AXIsProcessTrusted() else {
        fputs("Dock overlay: Accessibility access is unavailable; refusing input.\n", stderr)
        return nil
    }
    let system = AXUIElementCreateSystemWide()
    AXUIElementSetMessagingTimeout(system, 1)
    var element: AXUIElement?
    let error = AXUIElementCopyElementAtPosition(system, Float(point.x), Float(point.y), &element)
    guard error == .success, let element else {
        fputs("Dock overlay: Accessibility hit-test failed (\(error.rawValue)); refusing input.\n", stderr)
        return nil
    }
    var pid: pid_t = 0
    guard AXUIElementGetPid(element, &pid) == .success, pid > 0 else { return nil }
    return pid
}
if let result, let pid = result[kCGWindowOwnerPID as String] as? Int32 {
    print(pid, result[kCGWindowOwnerName as String] as? String ?? "?")
}
#endif

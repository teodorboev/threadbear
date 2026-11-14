// An external check during idle CPU measurements. No AX requests or app
// automation: those would wake the process whose CPU we're measuring.
import AppKit
import CoreGraphics
import Foundation

let pid = pid_t(CommandLine.arguments[1])!

final class OwnerPipe {
    private let lock = NSLock()
    private var ended = false
    func close() { lock.lock(); ended = true; lock.unlock() }
    var closed: Bool { lock.lock(); defer { lock.unlock() }; return ended }
}
let owner = OwnerPipe()
DispatchQueue.global().async {
    _ = FileHandle.standardInput.readDataToEndOfFile()
    owner.close()
}

func bounds(_ row: [String: Any]) -> CGRect? {
    guard let b = row[kCGWindowBounds as String] as? [String: Double],
          let x = b["X"], let y = b["Y"], let width = b["Width"], let height = b["Height"] else { return nil }
    return CGRect(x: x, y: y, width: width, height: height)
}

var windowID: CGWindowID?
var windowFrame = CGRect.zero
var refreshRates = Set<Double>()
func problem() -> String? {
    guard let session = CGSessionCopyCurrentDictionary() as? [String: Any],
          session[kCGSessionOnConsoleKey as String] as? Bool == true,
          session["CGSSessionScreenIsLocked"] as? Bool != true else { return "Console session is locked or switched away" }
    let front = NSWorkspace.shared.frontmostApplication
    guard front?.processIdentifier == pid else { return "App lost foreground role to \(front?.localizedName ?? "unknown") (pid \(front?.processIdentifier ?? 0)); expected \(pid)" }
    guard let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]],
          let index = rows.firstIndex(where: { ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 && (windowID == nil || ($0[kCGWindowNumber as String] as? CGWindowID) == windowID) }),
          let frame = bounds(rows[index]), frame.width >= 300, frame.height >= 300 else { return "App window is absent" }
    guard let foundID = rows[index][kCGWindowNumber as String] as? CGWindowID else { return "App window has no identity" }
    windowID = foundID
    windowFrame = frame
    var displays = [CGDirectDisplayID](repeating: 0, count: 32)
    var count: UInt32 = 0
    guard CGGetDisplaysWithRect(frame, UInt32(displays.count), &displays, &count) == .success, count > 0,
          displays.prefix(Int(count)).allSatisfy({ CGDisplayIsAsleep($0) == 0 }) else { return "App display is asleep or unavailable" }
    let activeFrames = displays.prefix(Int(count)).map { CGDisplayBounds($0) }
    for display in displays.prefix(Int(count)) {
        refreshRates.insert(CGDisplayCopyDisplayMode(display)?.refreshRate ?? 0)
    }
    let dock = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.dock")
        .first { $0.bundleURL?.path == "/System/Library/CoreServices/Dock.app" }?.processIdentifier
    let notificationPIDs = Set(NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.notificationcenterui")
        .filter { $0.bundleURL?.path == "/System/Library/CoreServices/NotificationCenter.app" }
        .map { $0.processIdentifier })
    // macOS can keep an invisible screen-sized Dock sentinel above all apps.
    // It doesn't occlude rendering. Ordinary Dock windows still fail below.
    for row in rows.prefix(index) {
        guard (row[kCGWindowAlpha as String] as? Double ?? 1) > 0,
              let cover = bounds(row), cover.intersects(frame.insetBy(dx: 24, dy: 24)) else { continue }
        if let dock, row[kCGWindowOwnerPID as String] as? pid_t == dock,
           row[kCGWindowName as String] as? String == "Dock",
           row[kCGWindowLayer as String] as? Int == 20, activeFrames.contains(cover) { continue }
        if let owner = row[kCGWindowOwnerPID as String] as? pid_t, notificationPIDs.contains(owner),
           row[kCGWindowName as String] as? String == "Notification Center", activeFrames.contains(cover) { continue }
        return "App window covered by \(row[kCGWindowOwnerName as String] as? String ?? "another window") (pid \(row[kCGWindowOwnerPID as String] as? pid_t ?? 0), window \(row[kCGWindowNumber as String] as? CGWindowID ?? 0), layer \(row[kCGWindowLayer as String] as? Int ?? 0), bounds \(cover), app bounds \(frame))"
    }
    return nil
}

var samples = 0
let began = ProcessInfo.processInfo.systemUptime
var lastCheck = ProcessInfo.processInfo.systemUptime
var maxGapMs = 0.0
var thermalStates = Set<Int>()
var lowPowerModes = Set<Bool>()
repeat {
    let now = ProcessInfo.processInfo.systemUptime
    maxGapMs = max(maxGapMs, (now - lastCheck) * 1000)
    lastCheck = now
    if maxGapMs > 1000 {
        fputs("Window watcher could not keep up\n", stderr)
        exit(2)
    }
    if let failure = problem() {
        fputs("\(failure)\n", stderr)
        exit(2)
    }
    samples += 1
    thermalStates.insert(ProcessInfo.processInfo.thermalState.rawValue)
    if #available(macOS 12.0, *) {
        lowPowerModes.insert(ProcessInfo.processInfo.isLowPowerModeEnabled)
    }
    if samples == 1 {
        print("ready"); fflush(stdout)
        lastCheck = ProcessInfo.processInfo.systemUptime
    }
    // Keep NSWorkspace's foreground notifications current between checks.
    let nextCheck = ProcessInfo.processInfo.systemUptime + 0.25
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.25))
    // A run loop with no input source may return immediately. It must not
    // turn the observer into a busy loop competing with the app.
    let remaining = nextCheck - ProcessInfo.processInfo.systemUptime
    if remaining > 0 { Thread.sleep(forTimeInterval: remaining) }
} while !owner.closed
maxGapMs = max(maxGapMs, (ProcessInfo.processInfo.systemUptime - lastCheck) * 1000)
if maxGapMs > 1000 {
    fputs("Window watcher could not keep up\n", stderr)
    exit(2)
}
let report: [String: Any] = [
    "samples": samples,
    "seconds": ProcessInfo.processInfo.systemUptime - began,
    "maxGapMs": maxGapMs,
    "windowId": windowID!, "width": windowFrame.width, "height": windowFrame.height,
    "displayRefreshHz": refreshRates.sorted(),
    "thermalStates": thermalStates.sorted(),
    "lowPowerModes": lowPowerModes.sorted { !$0 && $1 }
]
let data = try JSONSerialization.data(withJSONObject: report, options: .sortedKeys)
print(String(data: data, encoding: .utf8)!)

// Posts trackpad gestures through the window server at a screen point, as a
// trackpad sends them: fingers touch, then movement, then lift (or momentum).
// They go through the app's real dispatch, which the automation's made-up
// events don't: the window under the pointer, every monitor, and scroll views
// that take the rest of a gesture for themselves. Moves the real pointer, so
// run it on Tinker Desk's desktop, never Mike's (AGENTS.md, "Tests that need
// the real screen"). Build with `swiftc -O -o post-gesture post-gesture.swift`.
//
// usage: post-gesture <x> <y> <hold ms at end> <token>...
//   dx,dy  a movement (fingers' way; right and down positive)
//   M      after the lift, 10 momentum events down the same way
import CoreGraphics
import Foundation
import AppKit

let args = CommandLine.arguments
let p = CGPoint(x: Double(args[1])!, y: Double(args[2])!)
let holdMs = UInt32(args[3])!
let tokens = Array(args[4...])
CGWarpMouseCursorPosition(p)
CGAssociateMouseAndMouseCursorPosition(1)
// A warp alone sends no move, and a row's hover needs one.
CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
usleep(350_000)
// With PG_GUARD (a program printing the pid of the window on top at `<x> <y>`,
// features/tools/real-screen.ts's window-at) and PG_PID, it looks again after
// the hover and touches nothing if that pid's window isn't the one under the
// fingers, and before every event after that stops (no more events, not even
// the lift) if a window has come up over the point since. A guard program
// that fails, hangs past 2 s or says nothing counts as changed. Exit 3 says so.
let env = ProcessInfo.processInfo.environment
// The Dock's whole-screen see-through window (the Dock's own, layer 20, named
// Dock, a display's size): the one window the guard can let a click through,
// and so the only one the fingers may be over besides the app.
let dockPID = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.dock")
    .first { $0.bundleURL?.path == "/System/Library/CoreServices/Dock.app" }?.processIdentifier
var displayIDs = [CGDirectDisplayID](repeating: 0, count: 32)
var displayCount: UInt32 = 0
let displays = CGGetActiveDisplayList(32, &displayIDs, &displayCount) == .success ? displayIDs.prefix(Int(displayCount)).map { CGDisplayBounds($0) } : []
// Notification Centre keeps a see-through window over the whole screen that
// only its banners take clicks in: skipped, but only when it's really
// Notification Centre's, not any window with that title.
let notificationPIDs = Set(NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.notificationcenterui")
    .filter { $0.bundleURL?.path == "/System/Library/CoreServices/NotificationCenter.app" }.map(\.processIdentifier))
func covering() -> [(number: Int, pid: Int32, overlay: Bool)] {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    return list.compactMap { w in
        guard let b = w[kCGWindowBounds as String] as? [String: Double], (w[kCGWindowAlpha as String] as? Double ?? 1) > 0,
              let pid = w[kCGWindowOwnerPID as String] as? Int32,
              !((w[kCGWindowName as String] as? String) == "Notification Center" && notificationPIDs.contains(pid)),
              let x = b["X"], let y = b["Y"], let wd = b["Width"], let h = b["Height"],
              CGRect(x: x, y: y, width: wd, height: h).contains(p) else { return nil }
        let overlay = dockPID != nil && pid == dockPID && w[kCGWindowLayer as String] as? Int == 20
            && w[kCGWindowName as String] as? String == "Dock" && displays.contains(CGRect(x: x, y: y, width: wd, height: h))
        return (w[kCGWindowNumber as String] as? Int ?? -1, pid, overlay)
    }
}
var want: Int32?
var through: Set<Int> = []
if let tool = env["PG_GUARD"], let pid = env["PG_PID"].flatMap({ Int32($0) }) {
    want = pid
    let run = Process(), out = Pipe()
    run.executableURL = URL(fileURLWithPath: tool)
    run.arguments = [String(Int(p.x.rounded())), String(Int(p.y.rounded()))]
    run.standardOutput = out
    let done = DispatchSemaphore(value: 0)
    run.terminationHandler = { _ in done.signal() }
    var said = ""
    if (try? run.run()) != nil {
        if done.wait(timeout: .now() + 2) == .timedOut { run.terminate() }
        else if run.terminationReason == .exit && run.terminationStatus == 0 {
            said = String(data: out.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        }
    }
    let over = covering()
    // Anything over the app there but the Dock's overlay came up between the two looks.
    guard said.split(separator: " ").first.flatMap({ Int32($0) }) == pid, let mine = over.firstIndex(where: { $0.pid == pid }),
          over[..<mine].allSatisfy({ $0.overlay }) else {
        fputs("post-gesture: the window at (\(Int(p.x)), \(Int(p.y))) isn't \(pid)'s any more; nothing posted\n", stderr)
        exit(3)
    }
    // Over the point but let through by the guard there (the Dock's see-through overlay).
    through = Set(over[..<mine].map(\.number))
}
func post(_ dx: Double, _ dy: Double, phase: Int64, momentum: Int64 = 0) {
    if let want, covering().first(where: { !through.contains($0.number) })?.pid != want {
        fputs("post-gesture: something came up over (\(Int(p.x)), \(Int(p.y))); stopped\n", stderr)
        exit(3)
    }
    let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 2, wheel1: 0, wheel2: 0, wheel3: 0)!
    cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
    cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: phase)
    cg.setIntegerValueField(.scrollWheelEventMomentumPhase, value: momentum)
    let inverted = NSEvent(cgEvent: cg)!.isDirectionInvertedFromDevice
    let x = inverted ? dx : -dx, y = inverted ? dy : -dy
    cg.setDoubleValueField(.scrollWheelEventPointDeltaAxis2, value: x)
    cg.setDoubleValueField(.scrollWheelEventFixedPtDeltaAxis2, value: x)
    cg.setDoubleValueField(.scrollWheelEventPointDeltaAxis1, value: y)
    cg.setDoubleValueField(.scrollWheelEventFixedPtDeltaAxis1, value: y)
    cg.location = p
    cg.post(tap: .cghidEventTap)
}
post(0, 0, phase: 128) // CGScrollPhaseMayBegin: fingers touched
usleep(30_000)
post(0, 0, phase: 1)
usleep(16_000)
var last = (0.0, 0.0)
for t in tokens where t != "M" {
    let m = t.split(separator: ",").map { Double($0)! }
    post(m[0], m[1], phase: 2); last = (m[0], m[1]); usleep(16_000)
}
if holdMs > 0 { usleep(holdMs * 1000) }
post(0, 0, phase: 4)
if tokens.contains("M") {
    usleep(16_000)
    post(last.0, last.1, phase: 0, momentum: 1)
    for i in 1...10 { usleep(16_000); post(last.0 * 0.8, last.1 * 0.8, phase: 0, momentum: i == 10 ? 3 : 2) }
}

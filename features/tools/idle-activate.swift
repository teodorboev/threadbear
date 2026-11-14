import AppKit
import Foundation

let args = CommandLine.arguments
func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8)); exit(1)
}
guard args.count == 3, let pid = Int32(args[1]), pid > 0 else { fail("Expected owned PID and executable path") }
let expected = URL(fileURLWithPath: args[2]).resolvingSymlinksInPath().standardizedFileURL
func owned() -> NSRunningApplication {
    guard let app = NSRunningApplication(processIdentifier: pid), !app.isTerminated,
          app.executableURL?.resolvingSymlinksInPath().standardizedFileURL == expected,
          app.activationPolicy != .prohibited else { fail("Owned app identity or activation policy changed") }
    return app
}
let app = owned()
let unhidden = app.unhide()
let requested = app.activate(options: [.activateAllWindows])
let until = Date().addingTimeInterval(5)
while Date() < until && (NSWorkspace.shared.frontmostApplication?.processIdentifier != pid || !app.isActive) {
    let next = Date().addingTimeInterval(0.05)
    _ = RunLoop.current.run(mode: .default, before: next)
    // A CLI run loop can return immediately when no source is installed.
    if next.timeIntervalSinceNow > 0 { Thread.sleep(forTimeInterval: next.timeIntervalSinceNow) }
}
_ = owned()
let front = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
let result: [String: Any] = ["ownedPid": pid, "executable": expected.path, "unhideRequested": unhidden, "activateRequested": requested, "frontmostPid": front, "isActive": app.isActive]
let json = String(data: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), encoding: .utf8)!
if front != pid || !app.isActive { fail(json) }
print(json)

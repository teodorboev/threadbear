import AppKit

let screen = CGRect(x: 0, y: 0, width: 1920, height: 1080)
let point = CGPoint(x: 150, y: 142)
let dockPID: Int32 = 10, sourcePID: Int32 = 20, otherPID: Int32 = 30
let notificationPID: Int32 = 40
func window(_ pid: Int32, _ owner: String, _ name: String = "", layer: Int = 0, frame: CGRect = screen, alpha: Double = 1) -> [String: Any] {
    [kCGWindowOwnerPID as String: pid, kCGWindowOwnerName as String: owner,
     kCGWindowName as String: name, kCGWindowLayer as String: layer,
     kCGWindowAlpha as String: alpha,
     kCGWindowBounds as String: ["X": Double(frame.minX), "Y": Double(frame.minY), "Width": Double(frame.width), "Height": Double(frame.height)]]
}
let dock = window(dockPID, "Dock", "Dock", layer: 20)
let source = window(sourcePID, "Source")
let other = window(otherPID, "Other")

func expect(_ label: String, _ windows: [[String: Any]], _ expected: Int32?, ax: Int32? = sourcePID, query: Bool = false, displays: [CGRect] = [screen], identity: Int32? = dockPID, at: CGPoint = point) {
    var queried = false
    let result = windowAt(windows, point: at, displays: displays, dockPID: identity, notificationPIDs: [notificationPID]) {
        queried = true
        return ax
    }
    precondition(result?[kCGWindowOwnerPID as String] as? Int32 == expected, label)
    precondition(queried == query, "\(label): unexpected AX query")
}

expect("proved pass-through Dock", [dock, source], sourcePID, query: true)
expect("AX unavailable fails closed", [dock, source], dockPID, ax: nil, query: true)
expect("interactive Dock fails closed", [dock, source], dockPID, ax: dockPID, query: true)
expect("AX disagreement fails closed", [dock, source], dockPID, ax: otherPID, query: true)
expect("unknown Dock identity fails closed", [dock, source], dockPID, identity: nil)
expect("unknown display bounds fail closed", [dock, source], dockPID, displays: [])
expect("right Dock reveal edge stays blocked", [dock, source], dockPID, at: CGPoint(x: 1919, y: 500))
expect("left Dock reveal edge stays blocked", [dock, source], dockPID, at: CGPoint(x: 1, y: 500))
expect("bottom Dock reveal edge stays blocked", [dock, source], dockPID, at: CGPoint(x: 500, y: 1079))
expect("top edge stays blocked", [dock, source], dockPID, at: CGPoint(x: 500, y: 1))
expect("no window beneath Dock", [dock], dockPID)
expect("cannot skip a real window", [dock, other, source], dockPID, query: true)
expect("an exposed other app is still the target", [dock, other, source], otherPID, ax: otherPID, query: true)
expect("unknown overlay stays blocking", [window(otherPID, "Overlay", layer: 22), dock, source], otherPID)
expect("overlay beneath Dock stays blocking", [dock, window(otherPID, "Overlay", layer: 19), source], dockPID)
expect("Mission Control stays blocking", [window(dockPID, "Dock", "Mission Control", layer: 20), source], dockPID)
expect("another Dock layer stays blocking", [window(dockPID, "Dock", "Dock", layer: 25), source], dockPID)
expect("partial Dock stays blocking", [window(dockPID, "Dock", "Dock", layer: 20, frame: CGRect(x: 0, y: 0, width: 200, height: 1080)), source], dockPID)
expect("owner name alone is not Dock identity", [window(otherPID, "Dock", "Dock", layer: 20), source], otherPID)
expect("ordinary front window", [other, source], otherPID)
expect("transparent windows do not cover", [window(otherPID, "Overlay", alpha: 0), source], sourcePID)
expect("off-point windows do not cover", [window(otherPID, "Other", frame: CGRect(x: 500, y: 500, width: 100, height: 100)), source], sourcePID)
expect("existing Notification Center sentinel", [window(notificationPID, "Notification Centre", "Notification Center", layer: 21), dock, source], sourcePID, query: true)
expect("notification banners stay blocking", [window(notificationPID, "Notification Centre", "Banner", layer: 21), dock, source], notificationPID)
expect("another app cannot bypass by Notification Center title", [window(otherPID, "Other", "Notification Center", layer: 21), source], otherPID)
expect("another app cannot bypass by owner name and title", [window(otherPID, "Notification Centre", "Notification Center", layer: 21), source], otherPID)
expect("empty screen", [], nil)
print("screen guard checks passed")

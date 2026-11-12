// Real mouse events for selecting text, on Tinker Desk's desktop only (it
// takes the pointer: AGENTS.md, "Tests that need the real screen"). Used by
// features/tools/real-select.ts.
//
//   realselect drag <fromX> <fromY> <toX> <toY> <pressSeconds> <steps> <stepSeconds> <holdSeconds>
//     press and stay still `pressSeconds` (a hand often rests a moment before
//     it moves), then `steps` dragged events `stepSeconds` apart (a slow drag
//     is a few hundred of them over seconds), hold still, release
//   realselect click <x> <y> <count> [shift]
//     `count` presses in a row, as a double or triple click; with `shift`,
//     the Shift key held, as a click that extends a selection
// Screen points, from the top left.
import CoreGraphics
import Foundation

let a = CommandLine.arguments
func wait(_ s: Double) { Thread.sleep(forTimeInterval: s) }
func post(_ type: CGEventType, _ p: CGPoint, clicks: Int64 = 1, flags: CGEventFlags = []) {
    guard let e = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: p, mouseButton: .left) else { return }
    e.setIntegerValueField(.mouseEventClickState, value: clicks)
    e.flags = flags
    e.post(tap: .cghidEventTap)
}

switch a[1] {
case "drag":
    let from = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
    let to = CGPoint(x: Double(a[4])!, y: Double(a[5])!)
    let press = Double(a[6])!, steps = Int(a[7])!, step = Double(a[8])!, hold = Double(a[9])!
    post(.mouseMoved, from); wait(0.25)
    post(.leftMouseDown, from); wait(press)
    for i in 1...steps {
        let t = Double(i) / Double(steps)
        post(.leftMouseDragged, CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t))
        wait(step)
    }
    wait(hold)
    // Said first, so a script asking what's selected while the button's
    // down stops asking before the release can show the bar.
    print("releasing"); fflush(stdout)
    wait(0.05)
    post(.leftMouseUp, to); wait(0.3)
case "click":
    let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
    let count = Int64(a[4])!
    let shift = a.count > 5 && a[5] == "shift"
    let flags: CGEventFlags = shift ? .maskShift : []
    // The key itself as well as the flag on the clicks, as a hand holds it.
    func shiftKey(_ down: Bool) {
        let e = CGEvent(keyboardEventSource: nil, virtualKey: 56, keyDown: down)
        e?.flags = down ? .maskShift : []
        e?.post(tap: .cghidEventTap)
    }
    post(.mouseMoved, p); wait(0.2)
    if shift { shiftKey(true); wait(0.1) }
    for n in 1...count {
        post(.leftMouseDown, p, clicks: n, flags: flags); wait(0.05)
        post(.leftMouseUp, p, clicks: n, flags: flags); wait(0.08)
    }
    if shift { wait(0.05); shiftKey(false) }
    wait(0.3)
default:
    FileHandle.standardError.write("realselect drag|click ...\n".data(using: .utf8)!)
    exit(2)
}

// Real pointer moves or presses down a column of the screen, for
// hover-perf.ts: prints when it posted each one (CLOCK_UPTIME_RAW, the clock
// macOS stamps events with) and where, so the app's own log of when it
// handled each and painted the frame after can be joined to them. Moves the
// real pointer: Tinker Desk's desktop only, never Mike's (AGENTS.md).
//
//   pointer-probe move  <x> <fromY> <toY> <step> <every ms>
//   pointer-probe press <x> <fromY> <toY> <step> <every ms>
//
// Screen points from the top left. Each line out: "<ns> <x> <y>".
import CoreGraphics
import Foundation

let a = CommandLine.arguments
let mode = a[1]
let x = Double(a[2])!, from = Double(a[3])!, to = Double(a[4])!, step = abs(Double(a[5])!)
let every = useconds_t(Double(a[6])! * 1000)

func post(_ type: CGEventType, _ p: CGPoint) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
}

var out = ""
let dir: Double = to >= from ? 1 : -1
var y = from
// Settle on the first point, so the run starts from a known hover.
post(.mouseMoved, CGPoint(x: x, y: y))
usleep(400_000)
while dir > 0 ? y <= to : y >= to {
    let p = CGPoint(x: x, y: y)
    let ns = clock_gettime_nsec_np(CLOCK_UPTIME_RAW)
    if mode == "press" {
        post(.leftMouseDown, p)
        usleep(50_000)
        post(.leftMouseUp, p)
    } else {
        post(.mouseMoved, p)
    }
    out += "\(ns) \(x) \(y)\n"
    usleep(every)
    y += dir * step
}
print(out, terminator: "")

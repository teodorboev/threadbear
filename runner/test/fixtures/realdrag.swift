// Appended to app/Tools/realdrag.swift (built with -D TESTING): the drag's
// decisions against a made-up screen, recording what it would post.
import CoreGraphics

let app: Int32 = 100, target: Int32 = 555, alert: Int32 = 200, dock: Int32 = 10
let from = CGPoint(x: 400, y: 300), to = CGPoint(x: 800, y: 300)
let mine = [Over(number: 1, pid: app)]
let overlay = Over(number: 9, pid: dock, overlay: true)
let popup = Over(number: 7, pid: alert)

struct Run { var code: Int32; var sent: [Input] }
/// `owner` and `stack` answer from the steps posted so far: a window can come up mid-drag.
func run(_ f: CGPoint = from, _ t: CGPoint = to, pids: Pids? = Pids(press: app, drop: app),
         owner: @escaping (CGPoint, Int) -> Int32? = { _, _ in app },
         stack: @escaping (CGPoint, Int) -> [Over] = { _, _ in mine },
         front: Int32? = app) -> Run {
    var sent: [Input] = []
    var posted = 0
    let code = drag(from: f, to: t, hold: 0, press: 0, pids: pids,
                    look: Look(owner: { owner($0, posted) }, stack: { stack($0, posted) }, front: { front }),
                    post: { if case .move = $0 {} else { posted += 1 }; sent.append($0) }, wait: { _ in }, shoot: {}, say: { _ in })
    return Run(code: code, sent: sent)
}
func check(_ ok: Bool, _ label: String) { precondition(ok, label) }
func ups(_ r: Run) -> [CGPoint] { r.sent.compactMap { if case .up(let p) = $0 { return p } else { return nil } } }
func steps(_ r: Run) -> [CGPoint] { r.sent.compactMap { if case .dragged(let p) = $0 { return p } else { return nil } } }
let pressed = { (r: Run) in r.sent.contains { if case .down = $0 { return true } else { return false } } }

// Unguarded, as the other tools use it: the same events as ever.
var r = run(pids: nil, owner: { _, _ in nil }, stack: { _, _ in [] })
check(r.code == 0 && r.sent.count == 43 && r.sent.first == .move(from) && r.sent.last == .up(to), "unguarded drag unchanged")
// Guarded and clear all the way: the same events.
r = run()
check(r.code == 0 && r.sent.count == 43 && r.sent.last == .up(to) && !r.sent.contains(.escape), "a clear drag")

// The press: changed hands during the hover, the guard failed, or a window
// came up between the guard's answer and the window list.
r = run(owner: { _, _ in alert })
check(r.code == 3 && r.sent == [.move(from)], "press refused after the hover")
r = run(owner: { _, _ in nil })
check(r.code == 3 && r.sent == [.move(from)], "guard failure presses nothing")
r = run(stack: { _, _ in [popup] + mine })
check(r.code == 3 && r.sent == [.move(from)], "a window over the app at the press isn't trusted, whatever the guard said")
r = run(stack: { _, _ in [popup] })
check(r.code == 3 && !pressed(r), "no app window under the press")

// The Dock's overlay over everything, let through at the press: looked through all the way.
r = run(stack: { _, _ in [overlay] + mine })
check(r.code == 0 && r.sent.last == .up(to), "the Dock's overlay is looked through")
// Another window of the Dock's, not the overlay, is not.
r = run(stack: { p, _ in p.x > 600 ? [Over(number: 8, pid: dock)] + mine : [overlay] + mine })
check(r.code == 3 && ups(r) == [from], "another Dock window stops the drag")

// An alert comes up over the path after 10 steps: Escape (the app's in
// front), straight back to the press point, released there.
let late = { (p: CGPoint, n: Int) -> [Over] in n >= 11 && p.x > 650 ? [popup] + mine : mine }
r = run(stack: late)
check(r.code == 3 && r.sent.contains(.escape) && ups(r) == [from] && steps(r).last == from, "cancel jumps back to the press point")
check(!steps(r).contains { $0.x > 650 }, "no step over the alert")
// The alert's app came in front: no Escape for it.
r = run(stack: late, front: alert)
check(r.code == 3 && !r.sent.contains(.escape) && ups(r) == [from], "no Escape to another app")
// The press point is covered by then: released at the nearest clear step on the way.
r = run(stack: { p, n in n >= 11 && (p.x > 650 || p.x < 480) ? [popup] + mine : mine })
check(r.code == 3 && ups(r).count == 1 && ups(r)[0].x >= 480 && ups(r)[0].x <= 650 && steps(r).last == ups(r)[0], "released at the nearest clear step")
// The point it jumps back to is covered by the time it would let go: the next one.
var looks = 0
r = run(stack: { p, n in
    guard n >= 11 else { return mine }
    if p.x > 650 { return [popup] + mine }
    if p == from { looks += 1; return looks > 1 ? [popup] + mine : mine }
    return mine })
check(r.code == 3 && ups(r).count == 1 && ups(r)[0] != from, "a fresh look before letting go")
// Nowhere clear: let go where it is (the window server hands it to the app that had the press).
r = run(stack: { _, n in n >= 11 ? [popup] + mine : mine })
check(r.code == 3 && ups(r).count == 1, "nowhere clear still lets go once")

// The drop's owner changed (or the guard failed) at the end: cancelled, back to the press point.
r = run(owner: { p, _ in p == to ? nil : app })
check(r.code == 3 && r.sent.contains(.escape) && ups(r) == [from], "drop refused")
// A drop on our own target beside the app: steps over either are fine.
r = run(pids: Pids(press: app, drop: target), owner: { p, _ in p.x >= 600 ? target : app }, stack: { p, _ in p.x >= 600 ? [Over(number: 5, pid: target)] + mine : mine })
check(r.code == 0 && r.sent.last == .up(to), "drag to our drop target")

// A click: looked at before the press and before letting go.
r = run(from, from, owner: { _, _ in alert })
check(r.code == 3 && !pressed(r), "click refused")
r = run(from, from)
check(r.code == 0 && r.sent == [.move(from), .down(from), .up(from)], "a click")
r = run(from, from, stack: { _, n in n >= 1 ? [popup] + mine : mine }, front: alert)
check(r.code == 3 && !r.sent.contains(.escape) && ups(r).count == 1, "something over a click before it lets go")
// The jump back went to the press point, which was covered by the time it
// looked again, and nothing else is clear: it lets go where the pointer is,
// never back at the drop it jumped away from.
looks = 0
r = run(stack: { p, n in
    guard n >= 11 else { return mine }
    if p == from { looks += 1; return looks > 1 ? [popup] + mine : mine }
    return [popup] + mine }, front: alert)
check(r.code == 3 && ups(r) == [from] && steps(r).last == from, "lets go where the pointer is")
// The guard said the app at the drop, then a window came up there before letting go.
r = run(stack: { p, n in n >= 40 && p == to ? [popup] + mine : mine })
check(r.code == 3 && ups(r) == [from], "the drop's window list is looked at after the guard")
// A drag-and-drop's image (the app's own window) under the pointer at the
// drop, over our drop target: the guard says the app, and it's let go.
r = run(pids: Pids(press: app, drop: target), owner: { p, _ in p == to ? app : (p.x >= 600 ? target : app) },
        stack: { p, _ in p.x >= 600 ? [Over(number: 3, pid: app, layer: 500), Over(number: 5, pid: target)] + mine : mine })
check(r.code == 0 && r.sent.last == .up(to), "the app's drag image over the drop target")
// The same image, but over something else at the drop: cancelled.
r = run(pids: Pids(press: app, drop: target), owner: { p, _ in p == to ? app : (p.x >= 600 ? target : app) },
        stack: { p, _ in p == to ? [Over(number: 3, pid: app, layer: 500), popup, Over(number: 5, pid: target)] : (p.x >= 600 ? [Over(number: 5, pid: target)] + mine : mine) })
check(r.code == 3 && ups(r) == [from], "the drag image over an alert at the drop")
// Codex, round 5: the guard says the target, but the window list has a third
// app between the drag image and the target: cancelled.
r = run(pids: Pids(press: app, drop: target), owner: { p, _ in p.x >= 600 ? target : app },
        stack: { p, _ in p == to ? [Over(number: 3, pid: app, layer: 500), popup, Over(number: 5, pid: target)] : (p.x >= 600 ? [Over(number: 5, pid: target)] + mine : mine) })
check(r.code == 3 && ups(r) == [from], "a third app under the image at the drop, whatever the guard said")
// Codex, round 5b: dropping on the app itself, its drag image over a third
// app's window that has come up where the app's window was: cancelled.
r = run(owner: { _, _ in app }, stack: { p, _ in p == to ? [Over(number: 3, pid: app, layer: 500), popup] : mine })
check(r.code == 3 && ups(r) == [from], "a third app under the image when dropping on the app")
// And over the app's own window: let go.
r = run(owner: { _, _ in app }, stack: { p, _ in p == to ? [Over(number: 3, pid: app, layer: 500)] + mine : mine })
check(r.code == 0 && r.sent.last == .up(to), "the image over the app's own window")
// The same on the way: the image over a third app part way along stops the drag.
r = run(stack: { p, n in n >= 5 && p.x > 600 ? [Over(number: 3, pid: app, layer: 500), popup] : (n >= 5 ? [Over(number: 3, pid: app, layer: 500)] + mine : mine) })
check(r.code == 3 && !steps(r).contains { $0.x > 600 && $0 != from }, "a third app under the image on the way")
print("realdrag checks passed")

// A window that takes dropped files and says what came, for a real drag out of
// an app on Tinker Desk's desktop (features/tools/real-gestures.ts): a drop on
// the Finder would copy into whatever folder someone has open there.
//   droptarget <x> <y> <width> <height> <seconds>
// Screen points from the top left. Floats over other windows, prints "ready",
// then "dropped <path>" for each file let go over it, and quits after
// <seconds>. Build with `swiftc -O -o droptarget droptarget.swift`.
import AppKit

final class DropView: NSView {
    override init(frame: NSRect) {
        super.init(frame: frame)
        registerForDraggedTypes([.fileURL])
    }

    required init?(coder: NSCoder) { fatalError() }

    // Copy, as a drop on the Finder from another app is: nothing's moved.
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation { .copy }
    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation { .copy }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        let urls = sender.draggingPasteboard.readObjects(forClasses: [NSURL.self]) as? [URL] ?? []
        for url in urls { print("dropped \(url.path)") }
        fflush(stdout)
        return !urls.isEmpty
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemTeal.withAlphaComponent(0.6).setFill()
        dirtyRect.fill()
    }
}

let args = CommandLine.arguments
let (x, y, width, height, seconds) = (Double(args[1])!, Double(args[2])!, Double(args[3])!, Double(args[4])!, Double(args[5])!)
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let screenHeight = NSScreen.screens.first?.frame.height ?? 0
let window = NSWindow(contentRect: NSRect(x: x, y: screenHeight - y - height, width: width, height: height), styleMask: [.borderless], backing: .buffered, defer: false)
window.level = .floating
window.contentView = DropView(frame: NSRect(x: 0, y: 0, width: width, height: height))
window.orderFrontRegardless()
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { app.terminate(nil) }
print("ready")
fflush(stdout)
app.run()

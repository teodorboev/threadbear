// Pointer to glass, timed from outside the app, so the GPUI and Swift apps
// are measured the same way (hover-glass.ts): moves the real pointer onto
// each point in turn and records the screen with ScreenCaptureKit, and for
// each move says how long after it the pixels around that point first
// changed (a row's hover highlight coming on). Capture frames come at the
// screen's refresh, so each time is to within one refresh. Moves the real
// pointer and needs Screen Recording: Tinker Desk's desktop only (AGENTS.md).
//
//   hover-glass <x> <y0> <y1> <step> <settle ms>   (screen points, top left)
//
// Prints one line per move: "<y> <ms or -1>".
import AppKit
import CoreMedia
import ScreenCaptureKit

let a = CommandLine.arguments
let x = Double(a[1])!, y0 = Double(a[2])!, y1 = Double(a[3])!, step = Double(a[4])!, settle = Double(a[5])! / 1000

final class Watcher: NSObject, SCStreamOutput {
    let lock = NSLock()
    /// The point being watched (in the captured image's pixels), the pixels
    /// around it before the move, and when they first changed.
    var probe: (x: Int, y: Int)?
    var before: [UInt8]?
    var changedAt: UInt64?
    var armedAt: UInt64 = 0
    let scale: Double

    init(scale: Double) { self.scale = scale }

    func patch(_ buffer: CVPixelBuffer, _ px: Int, _ py: Int) -> [UInt8] {
        CVPixelBufferLockBaseAddress(buffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        let base = CVPixelBufferGetBaseAddress(buffer)!.assumingMemoryBound(to: UInt8.self)
        let row = CVPixelBufferGetBytesPerRow(buffer), w = CVPixelBufferGetWidth(buffer), h = CVPixelBufferGetHeight(buffer)
        var out: [UInt8] = []
        // A strip 60 px wide, 3 high, beside the pointer (not under it, so
        // the cursor itself doesn't count), sampled every 4th pixel.
        for dy in -1...1 {
            let yy = min(max(py + dy, 0), h - 1)
            for dx in stride(from: 24, to: 84, by: 4) {
                let xx = min(max(px + dx, 0), w - 1)
                let p = base + yy * row + xx * 4
                out.append(contentsOf: [p[0], p[1], p[2]])
            }
        }
        return out
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, let buffer = CMSampleBufferGetImageBuffer(sample) else { return }
        // When the frame was on the screen, on the host clock (mach time).
        let shown = CMClockConvertHostTimeToSystemUnits(CMSampleBufferGetPresentationTimeStamp(sample))
        lock.lock(); defer { lock.unlock() }
        guard let probe else { return }
        let now = patch(buffer, probe.x, probe.y)
        if before == nil { before = now; return }
        if changedAt == nil, shown >= armedAt, let before {
            let diff = zip(before, now).reduce(0) { $0 + abs(Int($1.0) - Int($1.1)) }
            if diff > now.count * 3 { changedAt = shown }
        }
    }
}

func machToMs(_ t: UInt64) -> Double {
    var tb = mach_timebase_info_data_t(); mach_timebase_info(&tb)
    return Double(t) * Double(tb.numer) / Double(tb.denom) / 1e6
}

func post(_ p: CGPoint) {
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
}

let done = DispatchSemaphore(value: 0)
Task {
  do {
    let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
    let display = content.displays.first { CGDisplayBounds($0.displayID).contains(CGPoint(x: x, y: y0)) } ?? content.displays[0]
    let bounds = CGDisplayBounds(display.displayID)
    let scale = Double(CGDisplayPixelsWide(display.displayID)) / bounds.width
    let config = SCStreamConfiguration()
    config.width = Int(bounds.width * scale); config.height = Int(bounds.height * scale)
    config.minimumFrameInterval = CMTime(value: 1, timescale: 120)
    config.pixelFormat = kCVPixelFormatType_32BGRA
    config.showsCursor = false
    config.queueDepth = 8
    let watcher = Watcher(scale: scale)
    let stream = SCStream(filter: SCContentFilter(display: display, excludingWindows: []), configuration: config, delegate: nil)
    try stream.addStreamOutput(watcher, type: .screen, sampleHandlerQueue: DispatchQueue(label: "frames"))
    try await stream.startCapture()
    var y = y0
    // Off the list first, so the first move is a real hover change.
    post(CGPoint(x: x - 200, y: y0)); try await Task.sleep(nanoseconds: 500_000_000)
    while y <= y1 {
        let px = Int((x - bounds.minX) * scale), py = Int((y - bounds.minY) * scale)
        watcher.lock.withLock { watcher.probe = (px, py); watcher.before = nil; watcher.changedAt = nil; watcher.armedAt = UInt64.max }
        // A refresh or two for the "before" pixels.
        try await Task.sleep(nanoseconds: 60_000_000)
        let at = mach_absolute_time()
        watcher.lock.withLock { watcher.armedAt = at }
        post(CGPoint(x: x, y: y))
        try await Task.sleep(nanoseconds: UInt64(settle * 1e9))
        let changed = watcher.lock.withLock { watcher.changedAt }
        print(String(format: "%.0f %.1f", y, changed.map { machToMs($0) - machToMs(at) } ?? -1))
        // Off the rows again, so the next move is a fresh hover.
        post(CGPoint(x: x - 200, y: y)); try await Task.sleep(nanoseconds: UInt64(settle * 1e9))
        y += step
    }
    try await stream.stopCapture()
    done.signal()
  } catch {
    // No Screen Recording, no display: say so rather than wait forever.
    FileHandle.standardError.write("hover-glass: \(error)\n".data(using: .utf8)!)
    exit(1)
  }
}
done.wait()

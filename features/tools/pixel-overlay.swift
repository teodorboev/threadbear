// Lays a Swift app picture and a GPUI app picture of the same scene over each
// other, to see where the two apps' layouts part (features/tools/compare.ts).
//
//   pixel-overlay overlay <swift.png> <gpui.png> <out.png>
//     The Swift picture's light in red, the GPUI one's in cyan: where both
//     draw the same thing in the same place it comes out grey, and an edge
//     that's off by a few points shows as a red and a cyan line side by side.
//   pixel-overlay edges <swift.png> <gpui.png> v <x> [y0 y1]
//   pixel-overlay edges <swift.png> <gpui.png> h <y> [x0 x1]
//     Where the brightness jumps going down column x (v) or along row y (h),
//     in each picture, in points, paired up nearest first, with the
//     difference: the numbers to change.
//   pixel-overlay rows <swift.png> <gpui.png> <x0> <x1> <y0> <y1> [threshold]
//   pixel-overlay cols <swift.png> <gpui.png> <y0> <y1> <x0> <x1> [threshold]
//     The runs of rows (or columns) with something drawn on them across a
//     band, in each picture, paired in order: a text line's top and bottom,
//     a button's, a separator's. The steadiest numbers for spacing.
//   pixel-overlay crop <in.png> <x> <y> <w> <h> <scale> <out.png>
//     A piece of a picture, made bigger, for looking at.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

struct Gray {
    let width: Int
    let height: Int
    var pixels: [UInt8]

    init(_ url: URL) {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            fatalError("can't read \(url.path)")
        }
        width = image.width
        height = image.height
        pixels = [UInt8](repeating: 0, count: width * height)
        // Drawn into a plain grey space: the Swift pictures carry the
        // display's colour profile and the GPUI ones none, so this puts both
        // on the same footing before comparing.
        let context = CGContext(data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    }

    subscript(x: Int, y: Int) -> Int { Int(pixels[y * width + x]) }
}

func write(_ data: inout [UInt8], width: Int, height: Int, to path: String) {
    let space = CGColorSpaceCreateDeviceRGB()
    let context = CGContext(data: &data, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4, space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    let image = context.makeImage()!
    let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    CGImageDestinationFinalize(dest)
}

/// A picture's points: both apps' pictures here are taken at the screen's
/// scale, so a 2x one is halved to compare in points.
func scale(_ a: Gray, _ b: Gray) -> (Double, Double) {
    (a.width >= 2000 ? 2 : 1, b.width >= 2000 ? 2 : 1)
}

func overlay(_ swiftPath: String, _ gpuiPath: String, _ out: String) {
    let a = Gray(URL(fileURLWithPath: swiftPath)), b = Gray(URL(fileURLWithPath: gpuiPath))
    let width = min(a.width, b.width), height = min(a.height, b.height)
    var data = [UInt8](repeating: 0, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let i = (y * width + x) * 4
            data[i] = UInt8(a[x, y])
            data[i + 1] = UInt8(b[x, y])
            data[i + 2] = UInt8(b[x, y])
            data[i + 3] = 255
        }
    }
    write(&data, width: width, height: height, to: out)
}

/// Brightness steps along a line: where it changes by more than `threshold`
/// from one pixel to the next, keeping one position per run of change.
func steps(_ line: [Int], threshold: Int = 14) -> [Int] {
    var out: [Int] = []
    var last = -10
    for i in 1..<line.count where abs(line[i] - line[i - 1]) > threshold {
        if i - last > 2 { out.append(i) }
        last = i
    }
    return out
}

func edges(_ swiftPath: String, _ gpuiPath: String, _ direction: String, _ at: Int, _ from: Int?, _ to: Int?) {
    let a = Gray(URL(fileURLWithPath: swiftPath)), b = Gray(URL(fileURLWithPath: gpuiPath))
    let (sa, sb) = scale(a, b)
    func line(_ g: Gray, _ s: Double) -> [Int] {
        let p = Int(Double(at) * s)
        if direction == "v" { return (0..<g.height).map { g[p, $0] } }
        return (0..<g.width).map { g[$0, p] }
    }
    let ea = steps(line(a, sa)).map { Double($0) / sa }, eb = steps(line(b, sb)).map { Double($0) / sb }
    let lo = Double(from ?? 0), hi = Double(to ?? 100_000)
    let ka = ea.filter { $0 >= lo && $0 <= hi }, kb = eb.filter { $0 >= lo && $0 <= hi }
    print("\(direction == "v" ? "down column x" : "along row y")=\(at): swift \(ka.count) edges, gpui \(kb.count)")
    print("  swift  gpui   gpui-swift")
    var used = Set<Int>()
    for e in ka {
        let best = kb.enumerated().filter { !used.contains($0.offset) }.min { abs($0.element - e) < abs($1.element - e) }
        if let best, abs(best.element - e) <= 24 {
            used.insert(best.offset)
            print(String(format: "  %5.0f  %5.0f  %+4.0f", e, best.element, best.element - e))
        } else {
            print(String(format: "  %5.0f      -", e))
        }
    }
    for (i, e) in kb.enumerated() where !used.contains(i) {
        print(String(format: "      -  %5.0f   (gpui only)", e))
    }
}

/// Where something is drawn across a band of the picture: going down (rows)
/// or along (cols) it, the runs of lines with anything on them other than
/// the band's background (its commonest shade), in points. Text lines, a
/// button's box and a separator each come out as one run, whatever the
/// columns or rows they start on, so two apps' lines can be paired in
/// order even where they don't start at the same x.
func ink(_ g: Gray, _ s: Double, across: Bool, _ lo: Int, _ hi: Int, _ from: Int, _ to: Int, threshold: Int) -> [(Double, Double)] {
    let (a0, a1) = (Int(Double(lo) * s), min(Int(Double(hi) * s), (across ? g.width : g.height) - 1))
    let (b0, b1) = (Int(Double(from) * s), min(Int(Double(to) * s), (across ? g.height : g.width) - 1))
    func value(_ along: Int, _ band: Int) -> Int { across ? g[along, band] : g[band, along] }
    var counts = [Int](repeating: 0, count: 256)
    for b in b0...b1 { for a in a0...a1 { counts[value(a, b)] += 1 } }
    let background = counts.indices.max { counts[$0] < counts[$1] }!
    var runs: [(Double, Double)] = []
    var start: Int?
    for b in b0...b1 {
        let marked = (a0...a1).contains { abs(value($0, b) - background) > threshold }
        if marked, start == nil { start = b }
        if !marked, let first = start {
            runs.append((Double(first) / s, Double(b) / s))
            start = nil
        }
    }
    if let first = start { runs.append((Double(first) / s, Double(b1 + 1) / s)) }
    return runs
}

func inkRuns(_ swiftPath: String, _ gpuiPath: String, across: Bool, _ lo: Int, _ hi: Int, _ from: Int, _ to: Int, _ threshold: Int) {
    let a = Gray(URL(fileURLWithPath: swiftPath)), b = Gray(URL(fileURLWithPath: gpuiPath))
    let (sa, sb) = scale(a, b)
    let ra = ink(a, sa, across: across, lo, hi, from, to, threshold: threshold), rb = ink(b, sb, across: across, lo, hi, from, to, threshold: threshold)
    print("\(across ? "rows" : "cols") \(from)-\(to) over \(across ? "x" : "y") \(lo)-\(hi): swift \(ra.count) runs, gpui \(rb.count)")
    print("  swift          gpui           start  end   size")
    for i in 0..<max(ra.count, rb.count) {
        let l = i < ra.count ? String(format: "%6.1f-%-6.1f", ra[i].0, ra[i].1) : String(repeating: " ", count: 13)
        let r = i < rb.count ? String(format: "%6.1f-%-6.1f", rb[i].0, rb[i].1) : String(repeating: " ", count: 13)
        var d = ""
        if i < ra.count, i < rb.count {
            d = String(format: "%+5.1f  %+5.1f  %+5.1f", rb[i].0 - ra[i].0, rb[i].1 - ra[i].1, (rb[i].1 - rb[i].0) - (ra[i].1 - ra[i].0))
        }
        print("  \(l)  \(r)  \(d)")
    }
}

func crop(_ path: String, _ x: Int, _ y: Int, _ w: Int, _ h: Int, _ s: Int, _ out: String) {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil), let image = CGImageSourceCreateImageAtIndex(source, 0, nil), let piece = image.cropping(to: CGRect(x: x, y: y, width: w, height: h)) else { fatalError("can't crop \(path)") }
    let context = CGContext(data: nil, width: w * s, height: h * s, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.interpolationQuality = .none
    context.draw(piece, in: CGRect(x: 0, y: 0, width: w * s, height: h * s))
    let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, context.makeImage()!, nil)
    CGImageDestinationFinalize(dest)
}

let args = CommandLine.arguments
switch args.count > 1 ? args[1] : "" {
case "overlay" where args.count == 5:
    overlay(args[2], args[3], args[4])
case "edges" where args.count >= 6:
    edges(args[2], args[3], args[4], Int(args[5])!, args.count > 6 ? Int(args[6]) : nil, args.count > 7 ? Int(args[7]) : nil)
case "crop" where args.count == 9:
    crop(args[2], Int(args[3])!, Int(args[4])!, Int(args[5])!, Int(args[6])!, Int(args[7])!, args[8])
case "rows" where args.count >= 8, "cols" where args.count >= 8:
    inkRuns(args[2], args[3], across: args[1] == "rows", Int(args[4])!, Int(args[5])!, Int(args[6])!, Int(args[7])!, args.count > 8 ? Int(args[8])! : 20)
default:
    print("usage: pixel-overlay overlay <swift.png> <gpui.png> <out.png> | edges <swift.png> <gpui.png> v|h <at> [from to] | rows|cols <swift.png> <gpui.png> <lo> <hi> <from> <to> [threshold] | crop <in.png> x y w h scale <out.png>")
    exit(2)
}

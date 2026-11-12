import AppKit
import XCTest
@testable import Threadbear

/// Where the slash palette goes against the window and the screen
/// (`SlashPaletteCoordinator.placement`). Screen coordinates, y up.
final class SlashPalettePlacementTests: XCTestCase {
    private let screen = NSRect(x: 0, y: 0, width: 1600, height: 1000)
    private typealias C = SlashPaletteCoordinator

    func testSitsAboveTheBoxWhenThereIsRoom() {
        let window = NSRect(x: 200, y: 50, width: 1000, height: 900)
        let box = NSRect(x: 300, y: 80, width: 800, height: 60)
        let placed = C.placement(box: box, window: window, content: window, screen: screen, width: 800, height: 400, gap: 8)
        XCTAssertEqual(placed.frame, NSRect(x: 300, y: 148, width: 800, height: 400))
    }

    func testStopsAtTheTopOfTheWindowsContent() {
        // A new thread's box, mid-window: the palette shrinks to the room
        // under the content's top rather than running off the window.
        let window = NSRect(x: 200, y: 50, width: 1000, height: 600)
        let content = NSRect(x: 200, y: 50, width: 1000, height: 572)
        let box = NSRect(x: 300, y: 300, width: 800, height: 60)
        let placed = C.placement(box: box, window: window, content: content, screen: screen, width: 800, height: 260, gap: 8)
        XCTAssertLessThanOrEqual(placed.frame.maxY, content.maxY - 8)
        XCTAssertEqual(placed.frame.minY, 368)
    }

    func testStopsAtTheTopOfTheScreen() {
        // Wayne Sutton's fork, 10 Nov: a window taller than the room left
        // on the screen, its box near the top, cut off the palette's first rows.
        let window = NSRect(x: 200, y: 300, width: 1000, height: 900)
        let box = NSRect(x: 300, y: 330, width: 800, height: 60)
        let placed = C.placement(box: box, window: window, content: window, screen: screen, width: 800, height: 900, gap: 8)
        XCTAssertLessThanOrEqual(placed.frame.maxY, screen.maxY - 8)
        XCTAssertGreaterThan(placed.frame.height, 0)
    }

    func testGoesUnderTheBoxWhenThereIsMoreRoomThere() {
        let window = NSRect(x: 200, y: 50, width: 1000, height: 900)
        let box = NSRect(x: 300, y: 850, width: 800, height: 60)
        let placed = C.placement(box: box, window: window, content: window, screen: screen, width: 800, height: 400, gap: 8)
        XCTAssertEqual(placed.frame.maxY, box.minY - 8)
        XCTAssertEqual(placed.frame.height, 400)
    }

    func testStaysOnTheScreenSideways() {
        // A window hanging off the screen's right edge.
        let window = NSRect(x: 1000, y: 50, width: 1000, height: 900)
        let box = NSRect(x: 1100, y: 80, width: 800, height: 60)
        let placed = C.placement(box: box, window: window, content: window, screen: screen, width: 800, height: 400, gap: 8)
        XCTAssertLessThanOrEqual(placed.frame.maxX, screen.maxX - 8)
        XCTAssertGreaterThanOrEqual(placed.frame.minX, window.minX + 8)
        XCTAssertLessThanOrEqual(placed.maxSize.width, screen.maxX - window.minX - 16)
    }
}

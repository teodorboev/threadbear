import AppKit
import BearDock
import SwiftUI
import XCTest
@testable import Threadbear

/// The transcript has one scroll view. Half dragging a panel drew a second
/// copy of it in the dock's ghost, which took the scroll view and left the
/// panel blank when the ghost went (Mike, 28 Oct).
@MainActor
final class TranscriptHostTests: XCTestCase {
    private func window(_ view: NSView) -> NSWindow {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 600, height: 400), styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        return window
    }

    func testTheDragGhostLeavesTheTranscriptInItsPanel() {
        let controller = TranscriptController()
        let panel = NSHostingView(rootView: TranscriptView(session: nil, controller: controller))
        let main = window(panel)
        panel.layoutSubtreeIfNeeded()
        XCTAssertTrue(controller.scrollView.window === main)

        let ghost = NSHostingView(rootView: TranscriptView(session: nil, controller: controller).environment(\.isDockGhost, true))
        let floating = window(ghost)
        ghost.layoutSubtreeIfNeeded()
        XCTAssertTrue(controller.scrollView.window === main, "the ghost took the transcript")

        floating.contentView = nil
        XCTAssertTrue(controller.scrollView.window === main)
    }

    func testAPanelTakesBackATranscriptLeftOffScreen() {
        let controller = TranscriptController()
        let host = TranscriptHost(controller: controller, ghost: false)
        let main = window(host)
        // Whatever took it then went away.
        let elsewhere = NSView()
        elsewhere.addSubview(controller.scrollView)
        XCTAssertNil(controller.scrollView.window)

        host.claim()
        XCTAssertTrue(controller.scrollView.window === main)
        XCTAssertEqual(controller.scrollView.frame, host.bounds)
    }

    /// Closing the Terminal (⌘J) rebuilds the dock: the new host takes the
    /// transcript, the outgoing one takes it back on its last update, then
    /// leaves the window. The transcript has to end up in the new host with
    /// no further update, or the thread shows blank (11 Nov).
    func testAHostLeavingTheWindowHandsTheTranscriptOn() {
        let controller = TranscriptController()
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 600, height: 400))
        let main = window(container)
        let outgoing = TranscriptHost(controller: controller, ghost: false)
        container.addSubview(outgoing)
        let incoming = TranscriptHost(controller: controller, ghost: false)
        container.addSubview(incoming)
        XCTAssertTrue(controller.scrollView.superview === incoming)

        outgoing.claim()
        XCTAssertTrue(controller.scrollView.superview === outgoing)
        outgoing.removeFromSuperview()

        XCTAssertTrue(controller.scrollView.superview === incoming, "the transcript left with the outgoing host")
        XCTAssertTrue(controller.scrollView.window === main)
    }
}

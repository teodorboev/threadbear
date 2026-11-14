// The system's translation popover, anchored in the GPUI view. SwiftUI is
// used only for the public translationPresentation API; it owns no editor.
import AppKit
import SwiftUI
import Translation

@available(macOS 14.4, *)
@MainActor private final class TranslationRequest: ObservableObject {
    @Published var presented = false
    let request: UInt64
    let words: String
    let editable: Bool
    private var delivered = false
    weak var host: NSView?
    init(request: UInt64, words: String, editable: Bool) {
        self.request = request
        self.words = words
        self.editable = editable
    }
    deinit {
        if !delivered {
            let id = request
            if Thread.isMainThread { tb_services_discard(id) }
            else { DispatchQueue.main.async { tb_services_discard(id) } }
        }
    }
    func answer(_ text: String) {
        guard editable, !delivered else { return }
        let bytes = Array(text.utf8)
        guard bytes.count <= 1024 * 1024 else { return }
        delivered = bytes.withUnsafeBufferPointer { tb_services_return(request, $0.baseAddress, $0.count) }
    }
    func close() { host?.removeFromSuperview() }
}

@available(macOS 14.4, *)
private struct TranslationAnchor: View {
    @ObservedObject var request: TranslationRequest
    var body: some View {
        Color.clear
            .translationPresentation(isPresented: $request.presented, text: request.words,
                replacementAction: request.editable ? { request.answer($0) } : nil)
            .onAppear { DispatchQueue.main.async { request.presented = true } }
            .onChange(of: request.presented) { _, up in if !up { request.close() } }
    }
}

@available(macOS 14.4, *)
private final class TranslationHost: NSHostingView<TranslationAnchor> {
    // The one-pixel popover anchor never takes a press meant for GPUI.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

// Owns the temporary retain from Rust on every path. The selected words and
// request identity are frozen before leaving GPUI; replacement is still
// checked there against the focused input, original range and original text.
@_cdecl("tb_selection_translate")
@MainActor func showTranslation(_ retained: UnsafeMutableRawPointer?, _ requestID: UInt64,
    _ bytes: UnsafePointer<UInt8>?, _ length: Int, _ x: Double, _ y: Double, _ editable: Bool) -> Bool {
    guard let retained else { tb_services_discard(requestID); return false }
    let view = Unmanaged<NSView>.fromOpaque(retained).takeRetainedValue()
    guard Thread.isMainThread, let bytes, length > 0, length <= 1024 * 1024,
          view.window != nil,
          let words = String(bytes: UnsafeBufferPointer(start: bytes, count: length), encoding: .utf8),
          #available(macOS 14.4, *) else { tb_services_discard(requestID); return false }
    // Only one translation popover in a window at once. Removing its anchor
    // dismisses the older one and discards its untouched request.
    for old in view.subviews where old is TranslationHost { old.removeFromSuperview() }
    let request = TranslationRequest(request: requestID, words: words, editable: editable)
    let host = TranslationHost(rootView: TranslationAnchor(request: request))
    request.host = host
    host.frame = NSRect(x: x, y: view.isFlipped ? y : view.bounds.height - y, width: 1, height: 1)
    view.addSubview(host)
    return true
}

import AppKit
import SwiftUI
import ThreadbearCore

/// The Swift app telling people about the GPUI app (`desktop/`), which is
/// replacing it, with one button that starts using it (Mike's pick, 13 Nov
/// 2026: mockups A and C in the GPUI port thread). Two pieces:
///
/// - A window over the main one, once, when the app starts (`NewAppWindow`):
///   what the new app is, that this one is frozen, "Not now" and "Start
///   using the new app".
/// - A card at the top of the sidebar (`NewAppCard`) whose "Start using it"
///   brings that window up again; "Later" hides it for a week. Once the
///   switch has started, the card shows its thread instead.
///
/// Starting doesn't run anything here: it starts a thread in the app's own
/// checkout with a brief (`brief(checkout:)`), as Investigate does, so an
/// agent runs `desktop/scripts/switch-from-swift.sh`, works out and fixes
/// whatever stops it (no Xcode tools, Rust, disk, a build error), and asks
/// before switching (Mike, 13 Nov: "a thread does it"). The script stays
/// the thread's tool, so the steps are the same whoever runs them.
///
/// Offered only while the GPUI app isn't installed, from an app that knows
/// its checkout and whose main has the script.
@MainActor
@Observable
final class NewAppOffer {
    static let shared = NewAppOffer()

    /// The window over the main one.
    var windowShown = false
    /// The thread's being made: the window's button spins and won't take
    /// another click.
    private(set) var starting = false
    /// Why the thread couldn't be made, in the window.
    private(set) var startError: String?
    /// Whether this Mac can be offered it at all.
    private(set) var offered = false
    /// The thread doing the switch, once there is one (kept across launches).
    private(set) var threadId: String?
    private var hiddenUntil: Date?
    private var checkout: String?
    @ObservationIgnored weak var app: AppModel?

    static let seenKey = "newAppPromptSeen"
    static let laterKey = "newAppCardHiddenUntil"
    static let threadKey = "newAppThreadId"
    static let target = URL(fileURLWithPath: NSHomeDirectory() + "/Applications/Threadbear GPUI.app")
    nonisolated static let script = "desktop/scripts/switch-from-swift.sh"
    /// Which features the new app has yet, from its behaviour specs.
    static let parityURL = URL(string: "https://github.com/mikecann/threadbear/blob/main/docs/gpui-port/PARITY.md")!

    /// The card: a switch under way, or offered and not put off with Later.
    var cardShown: Bool {
        if threadId != nil { return true }
        guard offered else { return false }
        return (hiddenUntil ?? .distantPast) < Date()
    }

    /// At launch, once the threads are in. A scripted run never shows the
    /// window by itself (the automation's `newApp` does).
    func setUp(app: AppModel, scripted: Bool) {
        self.app = app
        hiddenUntil = UserDefaults.standard.object(forKey: Self.laterKey) as? Date
        // A switch already started: its card stays until the thread's archived.
        if let id = UserDefaults.standard.string(forKey: Self.threadKey) {
            if let thread = app.threads[id], thread.archivedAt == nil {
                threadId = id
                return
            }
            UserDefaults.standard.removeObject(forKey: Self.threadKey)
        }
        guard !FileManager.default.fileExists(atPath: Self.target.path), let checkout = app.ownCheckout,
              FileManager.default.fileExists(atPath: checkout + "/.git") else { return }
        Task {
            // main has the script: a fork that hasn't brought it in yet
            // would only get an error.
            let result = await AppUpdater.run("/usr/bin/git", ["-C", checkout, "cat-file", "-e", "refs/heads/main:\(Self.script)"])
            guard result.status == 0 else { return }
            self.checkout = checkout
            self.offered = true
            // Once ever, and not over the tour, which has the window first.
            if !scripted, !UserDefaults.standard.bool(forKey: Self.seenKey), Tour.shared.step == nil {
                UserDefaults.standard.set(true, forKey: Self.seenKey)
                windowShown = true
            }
        }
    }

    func notNow() {
        windowShown = false
        startError = nil
    }

    func later() {
        let until = Date().addingTimeInterval(7 * 24 * 3600)
        hiddenUntil = until
        UserDefaults.standard.set(until, forKey: Self.laterKey)
    }

    /// Starts the thread that builds and switches. The window closes once
    /// it's made, and the card shows it.
    func start() {
        guard let app, let client = app.client, let checkout, !starting else { return }
        var request = app.newThreadRequest(text: Self.brief(checkout: checkout), images: [])
        request.cwd = checkout
        // It installs Rust, builds and moves an app into ~/Applications.
        request.permissionMode = .full
        request.title = "Switch to the new Threadbear app"
        starting = true
        startError = nil
        Task {
            defer { starting = false }
            do {
                let thread = try await client.createThread(request)
                app.threads[thread.id] = thread
                threadId = thread.id
                UserDefaults.standard.set(thread.id, forKey: Self.threadKey)
                windowShown = false
            } catch {
                startError = "The thread couldn't be started: \(error.localizedDescription)"
            }
        }
    }

    func openThread() {
        guard let app, let threadId else { return }
        app.select(threadId)
    }

    /// The first message of the thread that does the switch. Plain steps an
    /// agent can follow on someone else's Mac, with the script as its tool.
    nonisolated static func brief(checkout: String) -> String {
        let run = "f=\"$(mktemp)\"; git -C \"\(checkout)\" show refs/heads/main:\(script) > \"$f\"; taskpolicy -b /bin/bash \"$f\" \"\(checkout)\""
        return """
        I pressed Start using the new app in the Threadbear Mac app. Please install the new Threadbear desktop app on this Mac beside the current one, then ask me before switching to it. docs/guide.md, "Moving to the new desktop app", says what it is.

        1. Build and install it from what's committed on main, never the working tree, in the background with the longest timeout (the first build takes about 10 minutes, longer on a busy Mac; its STEP lines say where it's got to):

           \(run) --no-open

        2. If it fails, find out why from its output (also in ~/Library/Caches/Threadbear/update-desktop) and fix it, then run it again. The usual causes: Xcode's command line tools missing (`xcode-select --install` opens Apple's installer, which needs me to press Install), Rust (the script installs it with rustup if there's no cargo), too little disk space (it needs about 3 GB free), or a build error on main. Don't change the repo's code to get past a build error: tell me what failed. Exit 3 means another build of it is running, so wait and try again. Exit 5 means runner.json has check hosts but no updateBuildHosts, so ask me which Mac should build it.

        3. When ~/Applications/Threadbear GPUI.app is installed, end your turn asking me whether to switch now, with the answers Switch now and Later. Don't switch before I answer: I may be typing in this app.

        4. When I say Switch now, run the same command without --no-open:

           \(run)

           It skips the build, makes the new app the one threadbear:// links open, and opens it. The new app brings over this app's settings and drafts by itself, and this app stays installed. If I say Later, end the turn; I'll come back to this thread when I'm ready.
        """
    }

    /// For the automation's `newApp`: shows the pieces without a checkout or
    /// a thread behind them. {"window": true, "thread": "<id>", "error": "..."}
    func preview(_ c: [String: Any], app: AppModel) {
        self.app = app
        offered = true
        hiddenUntil = nil
        startError = c["error"] as? String
        threadId = c["thread"] as? String
        if let shown = c["window"] as? Bool { windowShown = shown }
    }
}

private enum NewAppCopy {
    static let windowTitle = "Try the new Threadbear app"
    static let windowBody = "It looks and works exactly like this one, and it runs on Windows and Linux as well as the Mac."
    /// When the Swift app stops being updated. Set it (say "1 December")
    /// once the GPUI app's remaining gaps and the carry-over of people's own
    /// Swift changes have landed, and both pieces say so.
    static let retirementDate: String? = nil
    static var frozen: String {
        let base = "This app no longer gets new features: everything new goes into the new app"
        return retirementDate.map { "\(base), and this one stops being updated on \($0)." } ?? "\(base)."
    }
    static let windowCarry = "It opens with your threads, settings and drafts, and this app stays installed so you can come back to it any time."
    static let windowBuild = "Starting it begins a thread that builds it on this Mac (about 10 minutes, installing Rust first if you need it), fixes anything that gets in the way, and asks you before it switches."
    static let windowGaps = "A few things still only work here."
    static let cardTitle = "A new Threadbear app"
    static let cardBody = "It looks and works like this one, and runs on Windows and Linux too. This one no longer gets new features."
}

@MainActor private var gpuiIcon: some View {
    Group {
        if let image = Art.gpuiApp {
            Image(nsImage: image).resizable().interpolation(.high)
        } else {
            RoundedRectangle(cornerRadius: 8).fill(Color.green.opacity(0.4))
        }
    }
}

/// Mockup A: over the main window, in the middle.
struct NewAppWindow: View {
    @Bindable var offer = NewAppOffer.shared

    var body: some View {
        if offer.windowShown {
            ZStack {
                // Clicks behind it go nowhere while it's up.
                Color.black.opacity(0.18).contentShape(Rectangle())
                    .onTapGesture {}
                card
                    .frame(width: zoomed(400))
                    .padding(zoomed(26))
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: zoomed(14), style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: zoomed(14), style: .continuous).strokeBorder(Color(nsColor: Theme.cardBorder), lineWidth: 0.5))
                    .shadow(color: .black.opacity(0.3), radius: zoomed(24), y: zoomed(8))
            }
            .transition(.opacity)
        }
    }

    private var card: some View {
        VStack(spacing: zoomed(12)) {
            gpuiIcon
                .frame(width: zoomed(72), height: zoomed(72))
                .clipShape(RoundedRectangle(cornerRadius: zoomed(14), style: .continuous))
            Text(NewAppCopy.windowTitle)
                .font(.system(size: TypeScale.scaled(17), weight: .semibold))
            VStack(spacing: zoomed(10)) {
                Text(NewAppCopy.windowBody)
                Text(NewAppCopy.frozen)
                Text(NewAppCopy.windowCarry)
                Text(NewAppCopy.windowBuild)
                    .foregroundStyle(.secondary)
                HStack(spacing: zoomed(4)) {
                    Text(NewAppCopy.windowGaps).foregroundStyle(.secondary)
                    Link("See which", destination: NewAppOffer.parityURL)
                        .tip("What the new app does so far, feature by feature")
                }
                if let error = offer.startError {
                    Text(error).foregroundStyle(.red)
                }
                HStack(spacing: zoomed(10)) {
                    Button("Not now") { offer.notNow() }
                        .keyboardShortcut(.cancelAction)
                        .buttonStyle(NewAppPrimary(quiet: true))
                        .tip("Close this. The card at the top of the sidebar can bring it back")
                    Button { offer.start() } label: {
                        HStack(spacing: zoomed(6)) {
                            if offer.starting { SpinnerMark(color: .white).frame(width: zoomed(12), height: zoomed(12)) }
                            Text("Start using the new app")
                        }
                    }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(NewAppPrimary())
                    .disabled(offer.starting)
                    .tip("Start a thread that installs the new app beside this one and asks you before switching")
                }
                .padding(.top, zoomed(6))
            }
            .font(.system(size: TypeScale.label))
            .fixedSize(horizontal: false, vertical: true)
        }
        .multilineTextAlignment(.center)
    }
}

/// Mockup C: at the top of the sidebar, above Starred.
struct NewAppCard: View {
    @Environment(AppModel.self) private var app
    @Bindable var offer = NewAppOffer.shared

    var body: some View {
        if offer.cardShown {
            VStack(alignment: .leading, spacing: zoomed(8)) {
                HStack(spacing: zoomed(10)) {
                    gpuiIcon
                        .frame(width: zoomed(30), height: zoomed(30))
                        .clipShape(RoundedRectangle(cornerRadius: zoomed(7), style: .continuous))
                    Text(offer.threadId == nil ? NewAppCopy.cardTitle : "Switching to the new app")
                        .font(.system(size: TypeScale.sidebarTitle, weight: .semibold))
                }
                if let id = offer.threadId {
                    switching(app.threads[id])
                } else {
                    Text(NewAppCopy.cardBody)
                        .font(.system(size: TypeScale.sidebarPreview))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    HStack(spacing: zoomed(8)) {
                        Button { offer.windowShown = true } label: {
                            Text("Start using it").frame(maxWidth: .infinity)
                        }
                        .buttonStyle(NewAppPrimary())
                        .tip("What the new app is, and a button that installs it")
                        Button("Later") { offer.later() }
                            .buttonStyle(.borderless)
                            .tip("Hide this for a week")
                    }
                }
            }
            .padding(zoomed(12))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: Theme.cardBackground), in: RoundedRectangle(cornerRadius: zoomed(10), style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: zoomed(10), style: .continuous).strokeBorder(Color(nsColor: Theme.cardBorder), lineWidth: 0.5))
            .padding(.horizontal, SidebarMetrics.outerInset)
            .padding(.top, zoomed(8))
            .transition(.opacity)
        }
    }

    /// The thread doing the switch: what it's up to, and Open.
    @ViewBuilder private func switching(_ thread: ThreadSummary?) -> some View {
        let (line, busy) = Self.status(thread)
        HStack(spacing: zoomed(6)) {
            if busy { SpinnerMark().frame(width: zoomed(12), height: zoomed(12)) }
            Text(line).font(.system(size: TypeScale.sidebarPreview)).foregroundStyle(.secondary)
        }
        Button { offer.openThread() } label: {
            Text("Open the thread").frame(maxWidth: .infinity)
        }
        .buttonStyle(NewAppPrimary(quiet: true))
        .tip("Open the thread that's installing the new app")
    }

    static func status(_ thread: ThreadSummary?) -> (String, Bool) {
        guard let thread else { return ("A thread is installing it", false) }
        if thread.needsYou { return ("It needs you", false) }
        if thread.status.isBusy { return ("Installing it", true) }
        if thread.status == .error { return ("It stopped with an error", false) }
        return ("Done for now: open it to see where it got to", false)
    }
}

/// The blue button in both mockups. `.borderedProminent` goes grey whenever
/// the window isn't the key one, which a window over the app at launch
/// often isn't.
private struct NewAppPrimary: ButtonStyle {
    /// The grey one ("Not now", "Open the thread").
    var quiet = false
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: TypeScale.label, weight: .medium))
            .foregroundStyle(quiet ? Color.primary : Color.white)
            .padding(.horizontal, zoomed(14))
            .padding(.vertical, zoomed(6))
            .background((quiet ? Color.primary.opacity(0.1) : Color(nsColor: .systemBlue)).opacity(configuration.isPressed ? 0.8 : enabled ? 1 : 0.5),
                        in: RoundedRectangle(cornerRadius: zoomed(7), style: .continuous))
    }
}

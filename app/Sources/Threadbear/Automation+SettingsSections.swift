#if DEBUG
import AppKit
import ThreadbearCore

// Settings › Runners, Phone & browser and Keyboard for the scripts and the
// specs (features/settings-*.feature), so they run on this app and the GPUI one
// alike: the `settings` command's actions on the first two (Keyboard's are the
// `shortcuts` command's), and what all three show in the dump. Settings is an
// AppKit form, and a scripted click on its controls starts a tracking loop
// that waits for a real mouse-up, so these do what the controls do instead.
// The GPUI app takes the same commands and gives the same dump. Debug builds
// only, like the rest of Automation.
extension Automation {
    /// The `settings` command's actions on these sections: `defaultRunner`
    /// (a runner's name, or "This Mac") as the picker sets it, `revoke` (a
    /// device's name) as its Revoke does, and `pairDone` as the code's Done.
    /// Nil when it asked for none of them, or when they went; an error otherwise.
    static func settingsSectionActions(_ c: [String: Any], app: AppModel) async -> String? {
        if let name = c["defaultRunner"] as? String {
            if name == "This Mac" {
                app.setDefaultRunner(nil)
            } else {
                guard let runner = app.remoteRunners.first(where: { $0.name == name }) else { return "error no runner \(name)" }
                app.setDefaultRunner(runner.id)
            }
        }
        if let name = c["revoke"] as? String {
            guard let device = app.devices.first(where: { $0.name == name }) else { return "error nothing paired called \(name)" }
            app.revoke(device)
        }
        if c["pairDone"] as? Bool == true {
            app.pairDoneRequest += 1
            for _ in 0..<200 where SettingsWindow.current?.attachedSheet != nil {
                try? await Task.sleep(for: .milliseconds(15))
            }
        }
        return nil
    }
}

extension AppDump {
    /// Settings › Runners as it shows: a line a runner in the order it lists
    /// them ("m5 *: This Mac", the star where new threads start), the
    /// default's name, whether the order can change, and the direct link.
    struct SettingsRunnersInfo: Encodable {
        var rows: [String]
        var `default`: String
        var canOrder: Bool
        var directLink: Bool
    }

    /// Settings › Phone & browser: a line a paired device, the first
    /// section's heading, or why nothing can show, the site, whether the list
    /// has come, whether the pairing code is up, and the last error. Never a token.
    struct SettingsDevicesInfo: Encodable {
        var rows: [String]
        var heading: String?
        var note: String?
        var site: String?
        var loaded: Bool
        var pairing: Bool
        var error: String?
    }

    /// Settings › Keyboard: a line a command ("Archive: ⌘K (changed)"),
    /// which one is listening, the question under it, the file's problem, and
    /// whether Reset All can be pressed.
    struct SettingsKeyboardInfo: Encodable {
        var rows: [String]
        var listening: String?
        var question: String?
        var canTake: Bool
        var problem: String?
        var canResetAll: Bool
    }

    @MainActor
    static func runnersInfo(_ app: AppModel) -> SettingsRunnersInfo {
        let selfName = app.runners.first(where: \.isSelf)?.name ?? "This Mac"
        func status(_ runner: RemoteRunner) -> String {
            if app.directRemoteLink, runner.tokenFile == nil { return "Not paired with this Mac" }
            return runner.online ? "Online" : "Offline"
        }
        func line(_ id: String?) -> String {
            if let id, let runner = app.remoteRunners.first(where: { $0.id == id }) {
                return runner.name + (app.defaultRunnerId == runner.id ? " *" : "") + ": " + status(runner)
            }
            return selfName + (app.defaultRunnerId == nil ? " *" : "") + ": This Mac"
        }
        let rows = app.canOrderRunners ? app.orderedRunnerIds.map { line($0) } : [line(nil)] + app.remoteRunners.map { line($0.id) }
        let defaultName = app.defaultRunnerId.map { id in app.remoteRunners.first { $0.id == id }?.name ?? id } ?? "This Mac"
        return SettingsRunnersInfo(rows: rows, default: defaultName, canOrder: app.canOrderRunners, directLink: app.directRemoteLink)
    }

    @MainActor
    static func devicesInfo(_ app: AppModel) -> SettingsDevicesInfo {
        let note: String?
        if app.devicesUnsupported {
            note = "This runner is too old to pair phones. It gets it when it next updates."
        } else if app.connection != .connected {
            note = "Not connected to the runner."
        } else if let reason = app.devicesReason {
            note = "Your threads aren't mirrored to Convex yet (\(reason)), so there's nothing for a phone or browser to read. Ask your agent to set it up, or run ./threadbear setup in the repo, then come back here."
        } else {
            note = nil
        }
        return SettingsDevicesInfo(
            rows: app.devices.map { "\($0.kind.rawValue) \"\($0.name)\": \(DeviceText.detail($0))" },
            heading: note == nil ? (app.localOnly ? "Add a browser" : "Add a phone or browser") : nil,
            note: note, site: app.devicesSite, loaded: app.devicesLoaded,
            pairing: SettingsWindow.current?.attachedSheet != nil, error: app.devicesError)
    }

    @MainActor
    static func keyboardInfo() -> SettingsKeyboardInfo {
        let shortcuts = AppShortcuts.shared
        let recorder = ShortcutRecorder.shared
        let rows = ShortcutGroup.allCases.flatMap(\.commands).map { command -> String in
            var line = "\(command.title): \(shortcuts.display(command) ?? "none")"
            if shortcuts.bindings.isChanged(command) { line += " (changed)" }
            if let key = shortcuts.key(command), let twin = shortcuts.bindings.command(using: key, except: command) { line += " (also \(twin.title))" }
            return line
        }
        let question = recorder.command == nil ? nil : ShortcutQuestion(recorder.outcome)
        return SettingsKeyboardInfo(rows: rows, listening: recorder.command?.title, question: question?.text, canTake: question?.canTake ?? false,
                                    problem: shortcuts.problem, canResetAll: shortcuts.bindings.anyChanged)
    }
}
#endif

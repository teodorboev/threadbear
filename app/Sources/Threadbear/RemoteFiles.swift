import AppKit
import ThreadbearCore

/// Pictures, long tool output and file links in another runner's thread
/// are paths on that Mac (docs/protocol.md, "Other runners' threads").
/// Through Convex the files stay on that machine: this Mac's runner would
/// read its own file at that path, which isn't the one meant. So they're
/// never opened here; the pane, the banner and the picture tiles say where
/// the file is instead. This Mac's own threads never come here: their
/// paths are local.
struct RemoteImages {
    /// "bruce", for the tile's words.
    let name: String
    /// Why the pictures don't show, for the tiles' tooltip.
    let why: String
}

extension AppModel {
    /// Why a file in `thread` (another runner's) can't be opened here.
    func remoteFileRefusal(_ thread: ThreadSummary) -> String {
        "This file is kept on \(runnerName(of: thread) ?? "the other machine")."
    }

    /// The picture source for a remote thread's transcript; nil for this
    /// Mac's own.
    func remoteImages(for thread: ThreadSummary) -> RemoteImages? {
        guard let name = runnerName(of: thread) else { return nil }
        return RemoteImages(name: name, why: remoteFileRefusal(thread))
    }

    /// The home folder on that Mac, for a `~/` path in its thread: from the
    /// thread's folder or that runner's default one when either is under
    /// /Users, else this Mac's (the runner expands `~` itself anyway).
    func remoteHome(of thread: ThreadSummary) -> String {
        let candidates = [thread.cwd, thread.runnerId.flatMap { remoteLogins[$0]?.defaults.cwd } ?? ""]
        for folder in candidates {
            let parts = folder.split(separator: "/", omittingEmptySubsequences: true)
            if parts.count >= 2, parts[0] == "Users" { return "/Users/" + parts[1] }
        }
        return NSHomeDirectory()
    }

    /// A file in another runner's thread, opened as `action` says: the
    /// pane says where it is, and the other ways of opening it say so in
    /// the banner. `from` is the section it opens over, when that isn't
    /// the one the thread would pick (the rail's Files panel).
    func openRemoteFile(_ url: URL, _ action: FileLinkAction, in thread: ThreadSummary, title: String? = nil, back: [PanePreview] = [], from: PaneSection? = nil) {
        let path = url.path
        let name = runnerName(of: thread) ?? "another Mac"
        let title = title ?? (path as NSString).lastPathComponent
        switch action {
        case .panel, .automatic:
            // The pane belongs to a thread; on a project page there's none.
            guard openProjectId == nil else { return openRemoteFile(url, .app, in: thread, title: title) }
            var preview = PanePreview(target: .remote(path: path, failed: remoteFileRefusal(thread)), title: title, from: from ?? paneSection(for: thread))
            preview.away = name
            preview.back = back
            show(preview, in: thread.id)
        case .app, .finder:
            errorMessage = "Couldn't get \(title) from \(name). \(remoteFileRefusal(thread))"
        }
    }
}

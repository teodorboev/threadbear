import Foundation

/// Files only this user's account may read, written whole: the composer's
/// drafts. As the GPUI app's `private_file.rs`: written into a new file of
/// its own beside the real one, made 0600 from the start (never written and
/// tightened after), under a name nobody can make first, then renamed over
/// the real one.
///
/// `Data.write(to:options: .atomic)` and a chmod after it left the drafts
/// readable by everyone (0644 under umask 022) from the write until the
/// chmod, and another account that opened the file in that moment kept
/// reading it (Daybreak, 11 Nov, findings 88 and 94 in the GPUI app, the same
/// in DraftStore).
public enum PrivateFile {
    public static func write(_ data: Data, to url: URL) throws {
        let folder = url.deletingLastPathComponent()
        if !FileManager.default.fileExists(atPath: folder.path) {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        }
        var temp = ""
        var fd: Int32 = -1
        for _ in 0..<16 {
            temp = folder.appendingPathComponent(".\(url.lastPathComponent).\(getpid())-\(UUID().uuidString).tmp").path
            // O_EXCL: never a file or symlink someone else made at that name.
            fd = open(temp, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
            if fd >= 0 || errno != EEXIST { break }
        }
        guard fd >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        var failure: Int32 = 0
        data.withUnsafeBytes { (buffer: UnsafeRawBufferPointer) in
            var offset = 0
            while offset < buffer.count {
                let wrote = Foundation.write(fd, buffer.baseAddress! + offset, buffer.count - offset)
                if wrote < 0 {
                    if errno == EINTR { continue }
                    failure = errno
                    return
                }
                offset += wrote
            }
        }
        if failure == 0, fsync(fd) != 0 { failure = errno }
        close(fd)
        if failure == 0, rename(temp, url.path) != 0 { failure = errno }
        if failure != 0 {
            unlink(temp)
            throw POSIXError(POSIXErrorCode(rawValue: failure) ?? .EIO)
        }
    }
}

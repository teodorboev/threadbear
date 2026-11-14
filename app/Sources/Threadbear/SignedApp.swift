import Foundation
import Security

/// Whether an application is the one it says it is, by its code signature,
/// before a fixed button or menu starts it. Any folder's .app can claim
/// another's bundle id with Launch Services (Finder and Spotlight register
/// what they see), so a lookup by id alone can hand back a cloned repo's
/// bundle, and starting it runs that repo's code (Daybreak 112, 13 Nov
/// 2026). The GPUI app has the same check (`signed_as` in open_gate.rs).
enum SignedApp {
    /// Google's Chrome: Apple's anchor for a Developer ID, Chrome's id and
    /// Google's team.
    static let googleChrome = #"anchor apple generic and identifier "com.google.Chrome" and certificate leaf[subject.OU] = "EQHXZ8M8AV""#
    static let installedApps = ["/Applications/", "/System/Applications/", "/Library/Developer/"]
    /// Apple's own Simulator, from Xcode.
    static let appleSimulator = #"anchor apple and identifier "com.apple.iphonesimulator""#

    /// The bundle at `url`, symlinks followed, if its signature is valid
    /// (strictly) and meets `requirement`; nil otherwise. Reads the bundle,
    /// a fraction of a second for Chrome, so call it off the main thread.
    static func checked(_ url: URL, _ requirement: String) -> URL? {
        let real = url.resolvingSymlinksInPath()
        // Only where apps are installed, which an agent confined to a project
        // can't write: the check and the launch each read the bundle at its
        // path, so a signed copy in a project folder could be swapped between
        // them (Codex's review of the fix). Chrome in ~/Applications goes to
        // the default browser.
        guard installedApps.contains(where: { real.path.hasPrefix($0) }) else { return nil }
        var code: SecStaticCode?
        var wanted: SecRequirement?
        guard SecStaticCodeCreateWithPath(real as CFURL, [], &code) == errSecSuccess, let code,
              SecRequirementCreateWithString(requirement as CFString, [], &wanted) == errSecSuccess, let wanted
        else { return nil }
        return SecStaticCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate), wanted) == errSecSuccess ? real : nil
    }
}

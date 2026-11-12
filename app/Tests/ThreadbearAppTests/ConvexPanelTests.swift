import WebKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// The Convex panel's script hands the deploy key to Convex's embedded
/// dashboard only when that page asks, and only on its origin. A page
/// standing in for it is loaded with the dashboard's origin as its base, so
/// no network is needed.
@MainActor
final class ConvexPanelTests: XCTestCase {
    private let key = ConvexPanelKey(deployment: "quick-fox-77", deploymentUrl: "https://quick-fox-77.convex.cloud", adminKey: "dev:quick-fox-77|it's \"quoted\"</script>")

    private func reply(origin: String) async throws -> String {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.userContentController.addUserScript(WKUserScript(
            source: WebPanelPool.convexCredentialsScript(key), injectionTime: .atDocumentStart, forMainFrameOnly: true))
        let view = WKWebView(frame: .zero, configuration: config)
        // What the embedded dashboard does: ask its parent (itself, at the
        // top level) and wait for the credentials.
        let page = """
        <script>
        window.got = 'none';
        window.addEventListener('message', (e) => { if (e.data && e.data.type === 'dashboard-credentials') window.got = JSON.stringify(e.data); });
        window.parent.postMessage({ type: 'dashboard-credentials-request' }, '*');
        </script>
        """
        view.loadHTMLString(page, baseURL: URL(string: origin)!)
        for _ in 0..<50 {
            try await Task.sleep(for: .milliseconds(100))
            if !view.isLoading, let got = try? await view.evaluateJavaScript("window.got") as? String, got != "none" { return got }
        }
        return (try? await view.evaluateJavaScript("window.got") as? String) ?? "none"
    }

    func testAnswersTheEmbeddedDashboardWithTheKey() async throws {
        let got = try await reply(origin: "https://dashboard-embedded.convex.dev/")
        let credentials = try JSONSerialization.jsonObject(with: Data(got.utf8)) as? [String: String]
        XCTAssertEqual(credentials, [
            "type": "dashboard-credentials", "adminKey": key.adminKey,
            "deploymentUrl": "https://quick-fox-77.convex.cloud", "deploymentName": "quick-fox-77",
        ])
    }

    func testSaysNothingOnAnyOtherOrigin() async throws {
        let got = try await reply(origin: "https://evil.example/")
        XCTAssertEqual(got, "none")
    }

    /// Codex review, 11 Nov: a compromised dashboard script setting
    /// `location` once a second opened a browser tab a second. Only a click
    /// or a form goes out; the rest is cancelled.
    func testOnlyAClickSendsTheDashboardToTheBrowser() {
        XCTAssertTrue(WebPanelPool.convexSendsOut(.linkActivated))
        XCTAssertTrue(WebPanelPool.convexSendsOut(.formSubmitted))
        for type: WKNavigationType in [.other, .reload, .backForward, .formResubmitted] {
            XCTAssertFalse(WebPanelPool.convexSendsOut(type), "navigation type \(type.rawValue)")
        }
    }

    /// And a script's own window.open, which WebKit allows on a Mac unless
    /// told not to, opens nothing: not from the dashboard, not from a
    /// patch's page.
    func testPanelPagesOpenNoWindowsOnTheirOwn() {
        let suite = "ConvexPanelTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let pool = WebPanelPool(defaults: defaults)
        let page = pool.webView(for: URL(string: "http://127.0.0.1:9/")!)
        XCTAssertFalse(page.configuration.preferences.javaScriptCanOpenWindowsAutomatically)
        let dashboard = pool.convexView(for: key)
        XCTAssertFalse(dashboard.configuration.preferences.javaScriptCanOpenWindowsAutomatically)
    }

    func testDeploymentChoiceIsRememberedPerConfiguredDeploymentAndUsedByBrowserLink() {
        let suite = "ConvexPanelTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let pool = WebPanelPool(defaults: defaults)
        let configured = ConvexDeployment(deployment: "quick-fox-77", deploymentUrl: "https://quick-fox-77.convex.cloud")
        XCTAssertEqual(pool.selectedConvex(configured), "quick-fox-77")
        pool.selectConvex("busy-bear-2", for: configured)
        XCTAssertEqual(WebPanelPool(defaults: defaults).selectedConvex(configured), "busy-bear-2")
        XCTAssertEqual(pool.convexBrowserURL(configured)?.absoluteString, "https://dashboard.convex.dev/d/busy-bear-2")
        let other = ConvexDeployment(deployment: "other-project-3", deploymentUrl: "https://other-project-3.convex.cloud")
        XCTAssertEqual(pool.selectedConvex(other), "other-project-3")
    }
}

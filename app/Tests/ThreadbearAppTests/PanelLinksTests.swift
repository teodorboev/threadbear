import Network
import WebKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// A patch's url panel, in a real web view (Mike, 11 Nov): a click on a
/// video in the YouTube patch's page loaded YouTube in the panel with no
/// way back, and closing the panel left the video playing.
@MainActor
final class PanelLinksTests: XCTestCase {
    func testWhichLinksLeaveThePanel() {
        let home = URL(string: "http://127.0.0.1:6427/")!
        func leaves(_ s: String, click: Bool = true, main: Bool = true) -> Bool {
            WebPanelPool.leavesPanel(URL(string: s), home: home, linkClick: click, mainFrame: main)
        }
        XCTAssertTrue(leaves("https://www.youtube.com/watch?v=x"))
        XCTAssertTrue(leaves("http://localhost:6427/"), "another host is another site")
        XCTAssertTrue(leaves("http://127.0.0.1:6428/"), "and another port")
        XCTAssertFalse(leaves("http://127.0.0.1:6427/video/1"), "the patch's own pages stay")
        XCTAssertFalse(leaves("HTTP://127.0.0.1:6427/x"))
        XCTAssertFalse(leaves("https://www.youtube.com/", click: false), "a redirect or a sign-in's script stays")
        XCTAssertFalse(leaves("https://www.youtube.com/", main: false), "an iframe's own links stay in it")
        XCTAssertFalse(leaves("mailto:a@b.c"), "only web pages")
        XCTAssertTrue(WebPanelPool.sameSite(URL(string: "https://a.com/x")!, URL(string: "https://a.com:443/y")!))
    }

    /// Serves the panel's page and a looping tone, on this Mac.
    private final class Site: @unchecked Sendable {
        let listener: NWListener
        var port: UInt16 { listener.port?.rawValue ?? 0 }
        var page = ""

        init() throws {
            listener = try NWListener(using: .tcp, on: .any)
            listener.newConnectionHandler = { [weak self] connection in
                connection.start(queue: .global())
                connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { data, _, _, _ in
                    let line = data.flatMap { String(data: $0, encoding: .utf8)?.components(separatedBy: "\r\n").first } ?? ""
                    // The other site's pages are plain, so one loaded in the
                    // panel doesn't run the panel page's scripts again.
                    let elsewhere = ["/watch", "/form", "/blank", "/popup"].contains { line.contains(" \($0)") }
                    let (type, body): (String, Data) = line.contains("/tone.wav")
                        ? ("audio/wav", Self.tone)
                        : ("text/html", Data((elsewhere ? "<!doctype html><title>elsewhere</title>" : self?.page ?? "").utf8))
                    var reply = Data("HTTP/1.1 200 OK\r\nContent-Type: \(type)\r\nContent-Length: \(body.count)\r\nConnection: close\r\n\r\n".utf8)
                    reply.append(body)
                    connection.send(content: reply, completion: .contentProcessed { _ in connection.cancel() })
                }
            }
        }

        func start() async throws {
            listener.start(queue: .global())
            for _ in 0..<50 where listener.port == nil || listener.state != .ready { try await Task.sleep(for: .milliseconds(20)) }
        }

        /// Two seconds of a quiet 440 Hz tone, 8 kHz mono 16-bit.
        static let tone: Data = {
            let rate = 8000, count = rate * 2
            var d = Data()
            func u32(_ v: Int) { var x = UInt32(v).littleEndian; d.append(Data(bytes: &x, count: 4)) }
            func u16(_ v: Int) { var x = UInt16(v).littleEndian; d.append(Data(bytes: &x, count: 2)) }
            d.append(Data("RIFF".utf8)); u32(36 + count * 2); d.append(Data("WAVEfmt ".utf8))
            u32(16); u16(1); u16(1); u32(rate); u32(rate * 2); u16(2); u16(16)
            d.append(Data("data".utf8)); u32(count * 2)
            for i in 0..<count {
                var s = Int16(sin(Double(i) * 2 * .pi * 440 / Double(rate)) * 2000).littleEndian
                d.append(Data(bytes: &s, count: 2))
            }
            return d
        }()

        deinit { listener.cancel() }
    }

    private func waitFor(_ what: String, _ check: () async -> Bool) async throws {
        for _ in 0..<100 {
            if await check() { return }
            try await Task.sleep(for: .milliseconds(100))
        }
        XCTFail("never: \(what)")
    }

    private func js(_ body: String, in view: WKWebView) async throws -> Any? {
        try await view.callAsyncJavaScript(body, contentWorld: .page)
    }

    func testLinksOffThePanelsSiteOpenOutsideAndItsOwnHaveBack() async throws {
        let site = try Site()
        try await site.start()
        let home = URL(string: "http://127.0.0.1:\(site.port)/")!
        let other = "http://localhost:\(site.port)"
        site.page = """
        <!doctype html><html><head><title>patch</title></head><body>
        <a id="video" href="\(other)/watch">video</a>
        <a id="blank" target="_blank" href="\(other)/blank">blank</a>
        <a id="own" href="/second">own</a>
        <form id="form" method="get" action="\(other)/form"><input name="q" value="x"></form>
        </body></html>
        """
        let pool = WebPanelPool(defaults: UserDefaults(suiteName: "PanelLinksTests.\(UUID().uuidString)")!)
        let view = pool.webView(for: home)
        var opened: [String] = []
        pool.setLeaving((home, { opened.append($0.absoluteString) }), for: view)
        try await waitFor("the page loads") { !view.isLoading && view.url == home }

        // A second apart: a panel sends out one page a second at most.
        let apart = Duration.milliseconds(1100)
        _ = try await js("document.getElementById('video').click()", in: view)
        try await waitFor("the video opens outside") { opened.contains("\(other)/watch") }
        try await Task.sleep(for: apart)
        _ = try await js("document.getElementById('blank').click()", in: view)
        try await waitFor("the new-tab link opens outside") { opened.contains("\(other)/blank") }
        try await Task.sleep(for: apart)
        // A button's window.open and a form to another site (Codex review).
        _ = try await js("window.open('\(other)/popup')", in: view)
        try await waitFor("window.open opens outside") { opened.contains("\(other)/popup") }
        try await Task.sleep(for: apart)
        _ = try await js("document.getElementById('form').submit()", in: view)
        try await waitFor("the form opens outside") { opened.contains { $0.hasPrefix("\(other)/form") } }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(view.url, home, "the patch's page stayed")
        XCTAssertEqual(opened.count, 4, "each opened once: \(opened)")
        XCTAssertFalse(pool.canGoBack(home))

        // Its own link loads in the panel, and Back comes back.
        _ = try await js("document.getElementById('own').click()", in: view)
        try await waitFor("its own page loads") { view.url?.path == "/second" && !view.isLoading }
        XCTAssertTrue(pool.canGoBack(home))
        pool.goBack(home)
        try await waitFor("Back") { view.url == home }
    }

    /// Closing the panel (or picking another thread) takes the view out of
    /// the window; the pool keeps it, and its sound stops.
    func testAClosedPanelsMediaPauses() async throws {
        let site = try Site()
        try await site.start()
        let home = URL(string: "http://127.0.0.1:\(site.port)/")!
        site.page = #"<!doctype html><html><body><audio id="a" loop src="/tone.wav"></audio></body></html>"#
        let pool = WebPanelPool(defaults: UserDefaults(suiteName: "PanelLinksTests.\(UUID().uuidString)")!)
        let view = pool.webView(for: home)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let container = WebPanelContainer(frame: window.contentView!.bounds)
        window.contentView!.addSubview(container)
        container.show(view)
        try await waitFor("the page loads") { !view.isLoading && view.url == home }
        _ = try await js("const a = document.getElementById('a'); a.muted = true; await a.play()", in: view)
        try await waitFor("it plays") { (try? await self.js("return document.getElementById('a').paused", in: view)) as? Bool == false }

        // Moving it to another panel's container doesn't pause it.
        let next = WebPanelContainer(frame: window.contentView!.bounds)
        window.contentView!.addSubview(next)
        next.show(view)
        try await Task.sleep(for: .milliseconds(300))
        let moved = try await js("return document.getElementById('a').paused", in: view) as? Bool
        XCTAssertEqual(moved, false)

        // Closing it does, and the page's own script can't start it again
        // unseen (Codex review).
        _ = try await js("const a = document.getElementById('a'); a.addEventListener('pause', () => setTimeout(() => a.play().catch(() => {}), 100))", in: view)
        next.removeFromSuperview()
        try await waitFor("it pauses") { (try? await self.js("return document.getElementById('a').paused", in: view)) as? Bool == true }
        try await Task.sleep(for: .milliseconds(800))
        let restarted = try await js("return document.getElementById('a').paused", in: view) as? Bool
        XCTAssertEqual(restarted, true, "the page started it again")
        window.close()
    }

    /// Codex review, 11 Nov: a page's own script could send tab after tab
    /// to the browser, its `a.click()` reading as a click and its
    /// `form.submit()` as a form. Only one a person clicked goes out, and
    /// then one a second. (A script run from here counts as a click, as
    /// WebKit runs it with a gesture; the page's own timers don't.)
    func testOnlyAPersonsClickSendsAPageOutAndOneASecond() async throws {
        XCTAssertTrue(WKNavigationAction.instancesRespond(to: NSSelectorFromString("_isUserInitiated")),
                      "WebKit's record of a person's click is gone, so nothing would leave a panel")
        let site = try Site()
        try await site.start()
        let home = URL(string: "http://127.0.0.1:\(site.port)/")!
        let other = "http://localhost:\(site.port)"
        func page(_ script: String) -> String {
            """
            <!doctype html><html><head><title>patch</title></head><body>
            <a id="video" href="\(other)/watch">video</a>
            <a id="blank" target="_blank" href="\(other)/blank">blank</a>
            <form id="form" method="get" action="\(other)/form"><input name="q" value="x"></form>
            <script>\(script)</script>
            </body></html>
            """
        }
        let pool = WebPanelPool(defaults: UserDefaults(suiteName: "PanelLinksTests.\(UUID().uuidString)")!)
        var opened: [String] = []
        // The page's own script, with no click behind it, each in a page of
        // its own as the first two take the page away: a link and a form to
        // another site (which load in the panel, as a sign-in's script
        // would), a new-tab link and a window.open (which WebKit holds back).
        for script in ["document.getElementById('video').click()", "document.getElementById('form').submit()",
                       "document.getElementById('blank').click()", "window.open('\(other)/popup')"] {
            site.page = page("setTimeout(() => \(script), 100);")
            let url = URL(string: "\(home)?\(UUID().uuidString)")!
            let view = pool.webView(for: url)
            pool.setLeaving((url, { opened.append($0.absoluteString) }), for: view)
            try await waitFor("the page loads") { !view.isLoading && view.url != nil }
            try await Task.sleep(for: .milliseconds(600))
        }
        XCTAssertEqual(opened, [], "a page's own script opened something")

        // Two clicks in a moment open one page, and one a second later another.
        site.page = page("")
        let view = pool.webView(for: home)
        pool.setLeaving((home, { opened.append($0.absoluteString) }), for: view)
        try await waitFor("the page loads") { !view.isLoading && view.url == home }
        _ = try await js("document.getElementById('blank').click()", in: view)
        _ = try await js("document.getElementById('video').click()", in: view)
        try await waitFor("one opens") { opened.count == 1 }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(opened, ["\(other)/blank"], "the second click within a second opened too")
        XCTAssertEqual(view.url, home, "the one too many still left the page where it was")
        try await Task.sleep(for: .milliseconds(1000))
        _ = try await js("document.getElementById('video').click()", in: view)
        try await waitFor("a second later, another") { opened.count == 2 }
    }

    /// Codex's second look (11 Nov): one click whose handler keeps going, a
    /// busy loop between its `a.click()`s, still had its gesture a second
    /// later, past the one-a-second gate. Each page that opens uses the
    /// click up, so one click opens one page.
    func testOneClickOpensOnePageHoweverLongItsHandlerRuns() async throws {
        XCTAssertTrue(WKNavigationAction.instancesRespond(to: NSSelectorFromString("_userInitiatedAction")),
                      "WebKit's click that can be used up is gone, so one click could open a page a second")
        let site = try Site()
        try await site.start()
        let home = URL(string: "http://127.0.0.1:\(site.port)/")!
        let other = "http://localhost:\(site.port)"
        site.page = "<!doctype html><title>patch</title><p>The patch's page.</p>"
        let pool = WebPanelPool(defaults: UserDefaults(suiteName: "PanelLinksTests.\(UUID().uuidString)")!)
        let view = pool.webView(for: home)
        var opened: [String] = []
        pool.setLeaving((home, { opened.append($0.absoluteString) }), for: view)
        try await waitFor("the page loads") { !view.isLoading && view.url == home }
        // One click (this runs with a gesture), three links out, in a new
        // tab and not, each more than a second after the last.
        _ = try await js("""
        for (let i = 0; i < 3; i++) {
          const a = document.createElement('a');
          a.href = '\(other)/watch?' + i;
          if (i % 2 === 0) a.target = '_blank';
          document.body.append(a);
          a.click();
          const until = performance.now() + 1200;
          while (performance.now() < until) {}
        }
        """, in: view)
        try await Task.sleep(for: .milliseconds(800))
        XCTAssertEqual(opened, ["\(other)/watch?0"], "one click opened more than one page")
        XCTAssertEqual(view.url, home, "and the page stayed")
    }

    /// The Convex dashboard's own view, which opens through the open gate
    /// rather than a patch's leaver, so the tests above don't reach it
    /// (Codex's second look, 11 Nov). Its own scripts open nothing, a click
    /// opens its link, a second click within a second doesn't, a clicked
    /// window.open does, and one click opens one page however long its
    /// handler runs. The page stands in for the dashboard on its origin, and
    /// its links go to the test's own site, which would load in the view if
    /// they weren't cancelled. (Not a closed port: WebKit stops a link to
    /// port 1 itself, before asking about it.)
    func testTheDashboardOpensOnlyWhatsClickedAndOnePageAClick() async throws {
        let saved = OpenGate.workspaceOpen
        defer { OpenGate.workspaceOpen = saved }
        var opened: [String] = []
        OpenGate.workspaceOpen = { opened.append($0.absoluteString) }
        let site = try Site()
        try await site.start()
        let docs = "http://localhost:\(site.port)/watch"
        let pool = WebPanelPool(defaults: UserDefaults(suiteName: "PanelLinksTests.\(UUID().uuidString)")!)
        let view = pool.convexView(for: ConvexPanelKey(deployment: "quick-fox-77", deploymentUrl: "https://quick-fox-77.convex.cloud", adminKey: "dev:quick-fox-77|key"))
        // The page's own script tries each way out with no click behind it,
        // a moment apart, so none takes another's place before it's been
        // asked about.
        view.loadHTMLString("""
        <!doctype html><title>dashboard</title>
        <a id="docs" href="\(docs)">docs</a>
        <a id="blank" target="_blank" href="\(docs)?blank">docs in a new tab</a>
        <form id="form" method="get" action="\(docs)"><input name="q" value="x"></form>
        <script>
        const steps = [() => document.getElementById('docs').click(), () => document.getElementById('form').submit(),
                       () => { location.href = '\(docs)?script'; }, () => document.getElementById('blank').click(),
                       () => open('\(docs)?open')];
        steps.forEach((step, i) => setTimeout(step, 100 + 300 * i));
        </script>
        """, baseURL: WebPanelPool.convexEmbedded)
        try await waitFor("the page loads") { !view.isLoading && view.url?.host == WebPanelPool.convexEmbedded.host }
        try await Task.sleep(for: .milliseconds(1900))
        XCTAssertEqual(opened, [], "the dashboard's own script opened something")

        _ = try await js("document.getElementById('docs').click()", in: view)
        try await waitFor("a click opens its link") { opened.count == 1 }
        _ = try await js("document.getElementById('blank').click()", in: view)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(opened, [docs], "a second click within a second opened too")

        try await Task.sleep(for: .milliseconds(1000))
        _ = try await js("window.open('\(docs)?window')", in: view)
        try await waitFor("a clicked window.open opens") { opened.count == 2 }

        try await Task.sleep(for: .milliseconds(1100))
        _ = try await js("""
        const ways = [() => document.getElementById('docs').click(), () => open('\(docs)?long'), () => document.getElementById('blank').click()];
        for (const go of ways) {
          go();
          const until = performance.now() + 1200;
          while (performance.now() < until) {}
        }
        """, in: view)
        try await Task.sleep(for: .milliseconds(800))
        XCTAssertEqual(opened, [docs, "\(docs)?window", docs], "one click opened more than one page")
        XCTAssertEqual(view.url?.host, WebPanelPool.convexEmbedded.host, "and the dashboard stayed")
    }

    /// And only a web page loads as a panel's page: a script's blob: or
    /// data: page doesn't (Codex review, 11 Nov).
    func testOnlyAWebPageLoadsAsAPanelsPage() async throws {
        let site = try Site()
        try await site.start()
        let home = URL(string: "http://127.0.0.1:\(site.port)/")!
        site.page = "<!doctype html><title>patch</title><p>The patch's page.</p>"
        let pool = WebPanelPool(defaults: UserDefaults(suiteName: "PanelLinksTests.\(UUID().uuidString)")!)
        let view = pool.webView(for: home)
        try await waitFor("the page loads") { !view.isLoading && view.url == home }
        _ = try await js("location.href = URL.createObjectURL(new Blob(['<title>blob</title>'], { type: 'text/html' }))", in: view)
        try await Task.sleep(for: .milliseconds(800))
        XCTAssertEqual(view.url, home, "a blob: page loaded in the panel")
        _ = try await js("location.href = 'data:text/html,<title>data</title>'", in: view)
        try await Task.sleep(for: .milliseconds(800))
        XCTAssertEqual(view.url, home, "a data: page loaded in the panel")
    }
}

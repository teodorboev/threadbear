Feature: Files opened in the side pane
  As someone reviewing what an agent made
  I want a file to open beside the conversation, drawn for what it is
  So I can read it without leaving the thread

  Background:
    Given the app is connected to a runner with the demo threads

  @file.host
  Scenario: A click on a file opens it over Highlights, and Back comes back
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    Then the pane is showing the file "docs/copy-review.md"
    When I press Back in the pane
    Then the pane shows Highlights

  # The Swift app is frozen (AGENTS.md, "The GPUI port"), so this is GPUI's only.
  @file.restore @swift-pending
  Scenario: A file open in the pane is open again after a relaunch, until Back closes it
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    Then the pane is showing the file "docs/copy-review.md"
    When the app is quit and opened again
    And I open the thread "Rebuild the three landing pages"
    Then the pane is showing the file "docs/copy-review.md"
    And the pane shows a "markdown" page with the words "Copy review: home, pricing, docs"
    When I press Back in the pane
    And the app is quit and opened again
    And I open the thread "Rebuild the three landing pages"
    Then the pane shows Highlights

  @file.markdown
  Scenario: A Markdown file is drawn as a page
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    Then the pane shows a "markdown" page with the words "Copy review: home, pricing, docs"
    And the pane's page has the words "Nine words, down from twenty."

  @file.markdown
  Scenario: A Markdown file's menu shows its own text, and the next one opens the same way
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    Then the pane's Markdown menu says "preview"
    When I pick "markdown" from the pane's Markdown menu
    Then the pane shows a "text" page with the words "# Copy review: home, pricing, docs"
    And the pane's Markdown menu says "markdown"
    When I press Back in the pane
    And I click the highlight "copy-review.md"
    Then the pane shows a "text" page with the words "## Home"
    When I pick "preview" from the pane's Markdown menu
    Then the pane shows a "markdown" page with the words "Nine words, down from twenty."

  @file.find
  Scenario: The find bar finds words on a drawn page, counts them and steps through them
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    And I look for "the" in the pane's file
    Then the find bar says "the: 1 of 8"
    When I find the next match
    And I find the next match
    Then the find bar says "the: 3 of 8"
    When I find the match before
    Then the find bar says "the: 2 of 8"
    When I look for "zzz" in the pane's file
    Then the find bar says "zzz: not found"
    When I close the find bar
    Then the find bar is gone

  @file.find
  Scenario: The find bar finds words in code whatever their case, and goes round
    When I open the thread "Rebuild the three landing pages"
    And I open "src/hero.ts" in the thread's folder in the pane
    And I look for "WORDS" in the pane's file
    Then the find bar says "WORDS: 1 of 2"
    When I find the next match
    Then the find bar says "WORDS: 2 of 2"
    When I find the next match
    Then the find bar says "WORDS: 1 of 2"

  @file.textview
  Scenario: A code file is shown as selectable, coloured text
    When I open the thread "Rebuild the three landing pages"
    And I open "src/hero.ts" in the thread's folder in the pane
    Then the pane shows a "code" page with the words "export function hero"

  @file.textview
  Scenario: A plain text file is shown as it is
    When I open the thread "Rebuild the three landing pages"
    And I open "logs/docs-build.txt" in the thread's folder in the pane
    Then the pane shows a "text" page with the words "Could not resolve image"

  # A video plays in the system's own player over the pane, its first frame
  # showing until it's played, sized to the film. The GPUI app does this on a
  # Mac; on Windows and Linux it still opens in its own app.
  @file.video @windows-pending @linux-pending
  Scenario: A video opens in the pane in a player, at its own size
    When I open the thread "Rebuild the three landing pages"
    And I open "screens/hero-walkthrough.mp4" in the thread's folder in the pane
    Then the pane is showing the file "screens/hero-walkthrough.mp4"
    And the pane shows a video 1600 by 1058

  # A panel in a window of its own can't place the player yet, so the video
  # stops and goes with the pane, and says it shows in the main window
  # (Codex's review, 12 Nov: it stayed up, unplaced, for good).
  @file.video @swift-pending @windows-pending @linux-pending
  Scenario: A video stops when its pane goes into a window of its own
    When I open the thread "Rebuild the three landing pages"
    And I open "screens/hero-walkthrough.mp4" in the thread's folder in the pane
    Then the pane shows a video 1600 by 1058
    When I put Highlights in a window of its own
    Then no video or PDF view is up

  # Quick Look draws a PDF in the Swift app; PDFKit's view does in the GPUI
  # app on a Mac.
  @file.quicklook @windows-pending @linux-pending
  Scenario: A PDF opens in the pane
    When I open the thread "Rebuild the three landing pages"
    And I open "docs/launch-brief.pdf" in the thread's folder in the pane
    Then the pane shows the PDF

  # Read-only Quick Look thumbnails on the Mac. Other systems still offer
  # the document's own app, through the open gate. Swift's frozen dump only
  # names its full Quick Look view; it cannot confirm a rendered thumbnail.
  @file.quicklook @swift-pending @windows-pending @linux-pending
  Scenario: An Office document previews without launching another app
    When I open the thread "Rebuild the three landing pages"
    And I open a sample Office document in the pane
    Then the pane shows a Quick Look thumbnail
    And the document has not opened another app

  # A link in a PDF goes through the app's open gate, as any link in a
  # thread does, never straight to the system: PDFKit's own handling would
  # open a file link to a program without asking (Codex's review, 12 Nov).
  # The Swift app's Quick Look view hands its links to the system itself.
  @file.quicklook @swift-pending @windows-pending @linux-pending
  Scenario: A link in a PDF in the pane opens through the app
    When I open the thread "Rebuild the three landing pages"
    And I open "docs/launch-brief.pdf" in the thread's folder in the pane
    And I click the link in the PDF in the pane
    Then "https://example.com/launch-brief" was opened through the app

  @file.picture
  Scenario: A picture is drawn in the pane, and Copy Image copies the picture itself
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "pricing-before.png"
    Then the pane shows a picture
    When I copy the picture in the pane
    Then it was copied as a picture

  # A local page shows in the pane, sealed: only itself and the assets it
  # names, no network, its scripts off until Run them for those bytes. The
  # GPUI app draws it with the web runtime; the dump's bar words, the page's
  # title and Run them are its own, so these wait on the Swift app's dump.
  @file.htmlpreview @web-runtime @swift-pending
  Scenario: A local page shows in the pane with its scripts off, and says it has some
    Given the web runtime is installed
    When I open the thread "Rebuild the three landing pages"
    And I open "mockups/drawn.html" in the thread's folder in the pane
    Then the pane shows a "web" page with the words "mockups/drawn.html"
    And the pane's page says "This page has scripts"
    And the local page in the pane is titled "Drawn mockup"

  @file.runthem @web-runtime @swift-pending
  Scenario: Run them runs the page's scripts, which still read only what it names
    Given the web runtime is installed
    When I open the thread "Rebuild the three landing pages"
    And I open "mockups/drawn.html" in the thread's folder in the pane
    And I press Run them over the page in the pane
    Then the local page in the pane is titled "named 200, unnamed 404, dotfile 404, away blocked, frame guarded yes, webrtc none"

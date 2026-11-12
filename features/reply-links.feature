Feature: Links in a reply
  As someone whose agents point me at files and pages
  I want each link to say where it goes and to offer the ways to open it
  So I can look before I click, and open it where I want it

  Background:
    Given the app is connected to a runner with the demo threads

  @rich.webmenu
  Scenario: A web link's right-click menu opens it, copies it and sets what a click does
    When I open the thread "Morning brief"
    And a reply linking "the release notes" to "https://example.com/notes" streams in
    Then the menu of the link "the release notes" offers "Open in Panel | Copy Link | Default"
    When I choose "Copy Link" from the menu of the link "the release notes"
    Then the clipboard holds "https://example.com/notes"

  @rich.filemenu
  Scenario: A file link's right-click menu opens it in the pane and copies its path
    Given a file "plan.md" saying "# The plan\n\nShip it." in a folder of its own
    When I open the thread "Morning brief"
    And a reply linking "the plan" to that file streams in
    Then the menu of the link "the plan" offers "Open in Panel | Show in Finder | Copy Path | Default" on a Mac, "Open in Panel | Show in Folder | Copy Path | Default" elsewhere
    When I choose "Copy Path" from the menu of the link "the plan"
    Then the clipboard holds that file's path
    When I choose "Open in Panel" from the menu of the link "the plan"
    Then the pane is showing that file

  @rich.filemenu
  Scenario: Default in a file link's menu sets what a click on a file link does
    Given a file "main.rs" saying "fn main() {}" in a folder of its own
    When I open the thread "Morning brief"
    And a reply linking "the code" to that file streams in
    And I pick "Open in Panel" under Default in the menu of the link "the code"
    And I click the link "the code"
    Then the pane is showing that file
    And nothing was opened in another app

  @rich.filerefs
  Scenario: A file a reply names by itself is found among the files the thread read, and a document opens in the pane
    Given a file "notes.md" saying "# Notes\n\nThe first line." in a folder of its own
    When I open the thread "Morning brief"
    And a reply that read that file and names it by itself in code streams in
    And I click the link "notes.md"
    Then the pane is showing that file
    And nothing was opened in another app

  @rich.cmdclick
  Scenario: A ⌘-click on a document opens it in its app, not the pane
    Given a file "notes.md" saying "# Notes" in a folder of its own
    When I open the thread "Morning brief"
    And a reply linking "the notes" to that file streams in
    And I ⌘-click the link "the notes"
    Then that file was opened in its app
    And the pane isn't showing that file

  @rich.linkpeek
  Scenario: Resting on a link shows where it goes
    Given a file "plan.md" saying "# The plan\n\nShip it." in a folder of its own
    When I open the thread "Morning brief"
    And a reply linking "the release notes" to "https://example.com/notes" and "the plan" to that file streams in
    Then the reply's links with a card are "the release notes" to "https://example.com/notes" and "the plan" to that file
    And resting on the link "the plan" shows its card
    And resting on the link "the release notes" shows its card

  # The same Default as a reply's web links, in Highlights' link rows.
  @highlights.rowmenus
  Scenario: A link row's menu sets what a click on a web link does
    When I open the thread "Rebuild the three landing pages"
    And I right-click the highlight "mikecann/site#48"
    Then its menu offers "Open in Panel | Copy link | Default | Remove"

  # A link that can't go anywhere says so, rather than looking like a
  # missed click (TranscriptController.openThreadLink). The Swift app does
  # both, but a scripted click on these links doesn't reach openThreadLink
  # there and its `beeps` counts only the card's; it's frozen (12 Nov), so
  # these are checked on the GPUI app.
  @rich.worklinks @swift-pending
  Scenario: A link to a thread that isn't in the sidebar beeps and says why
    When I open the thread "Morning brief"
    And a reply linking "the old thread" to "thread:0000dead" streams in
    And I click the link "the old thread"
    Then the app beeps
    And the error bar says "That thread isn't in your sidebar, so it can't open here. It was deleted, or it's archived on another machine, and those aren't listed on this Mac."

  @rich.worklinks @swift-pending
  Scenario: A link to a step that isn't there beeps
    When I open the thread "Morning brief"
    And a reply linking "↑" to "work:a%20step%20nobody%20took" streams in
    And I click the link "↑"
    Then the app beeps

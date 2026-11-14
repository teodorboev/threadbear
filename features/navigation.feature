Feature: Getting around the app
  As someone who lives in this app
  I want the same keys and menus as any good Mac app
  So my hands know where everything is

  Background:
    Given the app is connected to a runner with the demo threads

  @nav.history @nav.sidebuttons
  Scenario: The mouse's back and forward buttons walk through the places I've been
    When I open the thread "Morning brief"
    And I open the thread "Rename the Settings tabs"
    And I press the mouse's "back" button
    Then the open thread is "Morning brief"
    When I press the mouse's "forward" button
    Then the open thread is "Rename the Settings tabs"

  # A scripted Swift app's menu items keep their enabled state from the last
  # time the menu was opened (Back is grey until then), so a key that needs
  # an item enabled by the state change can only be pressed on the GPUI app.
  @nav.history @swift-pending
  Scenario: ⌘[ and ⌘] go back and forward
    When I open the thread "Morning brief"
    And I open the thread "Rename the Settings tabs"
    And I press "[" with "cmd"
    Then the open thread is "Morning brief"
    When I press "]" with "cmd"
    Then the open thread is "Rename the Settings tabs"

  @nav.nextinbox
  Scenario: Next Inbox Thread walks down the Inbox
    When I open the first thread in the Inbox
    And I press "down" with "cmd,opt"
    Then the second thread in the Inbox is open

  @nav.shortcuts
  Scenario: A key changed in shortcuts.json shows in the menu straight away
    When I open the thread "Morning brief"
    And shortcuts.json gives "archive" the key "cmd+y"
    Then the "Thread" menu has "Archive" on "⌘Y"

  # Pressing it is GPUI-only for the same reason as ⌘[ above (Archive's
  # enabled state goes stale in a scripted Swift app).
  @nav.shortcuts @swift-pending
  Scenario: A key changed in shortcuts.json works straight away
    When I open the thread "Morning brief"
    And shortcuts.json gives "archive" the key "cmd+y"
    Then the "Thread" menu has "Archive" on "⌘Y"
    When I press "y" with "cmd"
    Then the open thread isn't "Morning brief"

  @menu.file
  Scenario: The File menu starts a new thread
    Then the "File" menu has "New Thread" on "⌘N"

  @menu.view
  Scenario: The View menu has the zoom and panel items
    Then the "View" menu has "Zoom In" on "⌘="
    And the "View" menu has "Zoom Out" on "⌘-"
    And the "View" menu has "Actual Size" on "⌘0"
    And the "View" menu has "Reset Panel Layout"

  @menu.go
  Scenario: The Go menu has Back, Forward and Next Inbox Thread
    Then the "Go" menu has "Back" on "⌘["
    And the "Go" menu has "Forward" on "⌘]"
    And the "Go" menu has "Next Inbox Thread" on "⌥⌘↓"

  @menu.thread
  Scenario: The Thread menu has Archive, Stop and the views
    Then the "Thread" menu has "Archive" on "⌘E"
    And the "Thread" menu has "Stop" on "⌘."
    And the "Thread" menu has "Show Inbox" on "⌘1"
    And the "Thread" menu has "Show Projects" on "⌘3"

  @menu.help
  Scenario: The Help menu offers the tour
    Then the "Help" menu has "Take the Tour"

  @menu.app
  Scenario: The app menu opens Settings
    Then the app menu has "Settings…" on "⌘,"

  @menu.app
  Scenario: The app menu quits on the key every Mac app has
    Then the app menu has "Quit Threadbear" on "⌘Q" on a Mac, "Ctrl+Q" elsewhere

  # A scripted Swift app's arrow and Escape keys go only to a text box
  # (`arrow`, `escape`), and its dump says nothing of the list's keyboard or
  # what's scrolled into view, so the sidebar's keys are checked on the GPUI app.
  @sidebar.keys @swift-pending
  Scenario: Up and down move through the threads once the list is clicked
    When I click the row "Morning brief"
    Then the Threads panel has the keyboard
    When I press the down arrow
    Then the thread below "Morning brief" is open
    And the Threads panel has the keyboard
    When I press the up arrow
    Then the open thread is "Morning brief"

  @sidebar.keys @swift-pending
  Scenario: The arrow keys keep the open thread's row in view
    Given the window is short
    When I click the first row in the sidebar
    And I press the down arrow until the last thread in the sidebar is open
    Then the open thread's row is in view in the sidebar

  @sidebar.keys @swift-pending
  Scenario: Esc in the Scheduled view goes back to the Inbox
    When I click "Scheduled" in the activity bar
    Then the sidebar shows the "scheduled" view
    When I press Escape
    Then the sidebar shows the "inbox" view

  @sidebar.keys @nav.nextinbox @swift-pending
  Scenario: Next Inbox Thread brings the row it opens into view
    Given the window is short
    When I open the first thread in the Inbox
    And I go down the Inbox to its last thread with Next Inbox Thread
    Then the open thread's row is in view in the sidebar

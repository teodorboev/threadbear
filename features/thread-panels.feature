Feature: The panels round the thread
  As someone with several threads going at once
  I want to fold the thread list away and keep a thread open beside another
  So the window shows what I'm working on and nothing else

  Background:
    Given the app is connected to a runner with the demo threads

  @menu.view
  Scenario: Collapse Threads folds the list away, and ⌘/ brings it back as it was
    Then the "View" menu has "Collapse Threads" on "⌘/"
    When I choose "Collapse Threads" from the menu bar
    Then the Threads panel is folded away
    And the panels are laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"
    When I press "/" with "cmd"
    Then the Threads panel is showing

  # A scripted Swift app's menu items keep their titles from the last time
  # the menu was opened, so only the GPUI app can say them as they change.
  @menu.view @swift-pending
  Scenario: The View menu's items say what they'll do once the list is folded away
    When I choose "Collapse Threads" from the menu bar
    Then the "View" menu has "Expand Threads" on "⌘/"
    And the "View" menu has "Show Threads"
    When I choose "Expand Threads" from the menu bar
    Then the Threads panel is showing
    And the "View" menu has "Hide Threads"

  @menu.view
  Scenario: ⌘/ folds the list away, and the activity bar's Threads button brings it back
    When I press "/" with "cmd"
    Then the Threads panel is folded away
    When I click "Threads" in the activity bar
    Then the Threads panel is showing

  @menu.view
  Scenario: The folded list stays folded when the app is opened again
    When I press "/" with "cmd"
    Then the Threads panel is folded away
    When the app is quit and opened again
    Then the Threads panel is folded away

  @dock.threadpanel
  Scenario: Open in New Panel puts a thread beside the main one, and the main panel moves on
    When I open the thread "Morning brief"
    And I open "Morning brief" in a new panel
    Then "Morning brief" is in a panel of its own, showing its transcript
    And the new-thread page shows in the main panel
    When I open the thread "Rename the Settings tabs"
    Then the open thread is "Rename the Settings tabs"
    And "Morning brief" is in a panel of its own, showing its transcript

  @dock.threadpanel
  Scenario: The row's menu offers Open in New Panel, then Move to Main Panel
    When I right-click the row "Morning brief"
    Then the row's menu offers "Open in New Panel"
    When I open "Morning brief" in a new panel
    And I right-click the row "Morning brief"
    Then the row's menu offers "Move to Main Panel"

  @dock.threadpanel
  Scenario: Picking a thread that's in its own panel leaves the main panel as it is
    When I open "Morning brief" in a new panel
    And I open the thread "Rename the Settings tabs"
    And I pick "Morning brief" in the sidebar
    Then the open thread is "Rename the Settings tabs"
    And "Morning brief" is in a panel of its own, showing its transcript

  @dock.threadpanel
  Scenario: Move to Main Panel closes the thread's panel and shows it in the main one
    When I open "Morning brief" in a new panel
    And I move "Morning brief" to the main panel
    Then "Morning brief" isn't in a panel of its own
    And the open thread is "Morning brief"

  # Back goes through the same pick as a click on the row (the Swift app's
  # `select`), so it brings the thread's own panel forward.
  @dock.threadpanel
  Scenario: Back to a thread that's in its own panel leaves the main panel as it is
    When I open the thread "Morning brief"
    And I open the thread "Rename the Settings tabs"
    And I open "Morning brief" in a new panel
    And I press the mouse's "back" button
    Then the open thread is "Rename the Settings tabs"
    And "Morning brief" is in a panel of its own, showing its transcript

  # In both apps opening a thread in any way (its row, a link, Back) sends a
  # thread that has a panel of its own there, so no click leaves the main
  # panel with it selected. The note is the Swift app's safety net
  # (DetailView's `OpenElsewhereNote` branch), so the script puts the window
  # in that state (`open-panel` with `keepSelected`) to check what it says.
  @dock.openelsewhere
  Scenario: A main panel left with a thread that's in its own panel says so
    Given the main panel has "Morning brief" selected while it's in a panel of its own
    Then the main panel says "Morning brief" is open in its own panel
    When I press "Go to it" on the main panel's note
    Then "Morning brief" is in a panel of its own, showing its transcript
    When I press "Show it here" on the main panel's note
    Then "Morning brief" isn't in a panel of its own
    And the open thread is "Morning brief"

  @dock.contexttabs
  Scenario: A panel opened as a tab beside the thread closes when another thread is picked
    When I open the thread "Morning brief"
    And I'm looking at the app
    And the agent asks to show the "updates" panel as a tab
    Then the panels are laid out as "row[threads (no title) 29% | thread+updates* 47% | highlights 24%]"
    When I open the thread "Rename the Settings tabs"
    Then the panels are laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"

  @dock.contexttabs
  Scenario: A tab dragged away from the thread stays when another thread is picked
    When I open the thread "Morning brief"
    And I'm looking at the app
    And the agent asks to show the "updates" panel as a tab
    # The ask reaches the app through the runner, so the tab may not be
    # there yet: on GitHub's Windows the move came first ("updates isn't in
    # the layout", run 43399468986).
    Then the dock has the "updates" panel
    When I move the "updates" panel to the "left" edge
    And I open the thread "Rename the Settings tabs"
    Then the dock has the "updates" panel

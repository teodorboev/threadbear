Feature: Other runners' threads
  As someone with threads running on other machines
  I want them listed beside this Mac's, and to work with them here
  So it doesn't matter where a thread runs

  Background:
    Given the app is connected to a runner with the demo threads

  @sidebar.row.remote @remote-threads
  Scenario: Another runner's threads are listed with its machine
    Then the row "Check the photo library backup" is on the runner "On bruce"
    And the row "Check the photo library backup" has the machine tile
    And the row "Morning brief" isn't on another runner

  @sidebar.row.remote @remote-threads
  Scenario: Opening one shows its transcript
    When I open the thread "Tidy the media drive"
    Then the transcript shows my message "Find duplicate videos on the media drive."
    And the transcript shows a reply starting "Found 14 duplicates"
    And the composer is on the runner "bruce"

  # Its picture is a path on bruce: asked of the runner, which keeps another
  # Mac's files there for now, so the tile says it couldn't get it, and never
  # shows whatever this Mac has at that path.
  @transcript.images @remote-threads
  Scenario: Another Mac's picture says where it is rather than showing a file of this Mac's
    When I open the thread "Check the photo library backup"
    Then the pictures of my message say "Couldn't get it from bruce"

  # Its chip names the same file on bruce: pressed here it opens nothing, as
  # this Mac's file at that path isn't the one meant (Codex, 12 Nov).
  @rich.picturechips @remote-threads
  Scenario: Another Mac's picture chip opens nothing on this Mac
    When I open the thread "Check the photo library backup"
    Then pressing the picture chip in my message opens nothing

  @sidebar.row.remote @remote-threads
  Scenario: It takes a message like any other thread
    When I open the thread "Tidy the media drive"
    And I send "Keep the copies in Inbox for now"
    Then the transcript shows my message "Keep the copies in Inbox for now"

  @composer.runner @remote-threads
  Scenario: The new-thread page offers each machine, with the default starred
    When I start a new thread
    Then the "runner" menu has a row "[x] This Mac *"
    And the "runner" menu has a row "[ ] bruce"

  @composer.runner @remote-threads
  Scenario: Picking a machine is where the thread will start
    When I start a new thread
    And I pick "bruce" from the "runner" menu
    Then the composer is on the runner "bruce"
    And the note under the box says "Starts in misc on bruce, with mike"
    And the "runner" menu has a row "[x] bruce"

  @composer.runner @remote-threads
  Scenario: A new thread started on another machine is listed as its
    When I start a new thread
    And I pick "bruce" from the "runner" menu
    And I send "Check the backup drive's health"
    Then a thread called "Check the backup drive's health" opens
    And the transcript shows my message "Check the backup drive's health"
    And the row "Check the backup drive's health" is on the runner "On bruce"

  @sidebar.move @remote-threads
  Scenario: Move to offers the other machines from the row
    When I right-click the row "Morning brief"
    Then the row's menu offers "Move to"

  @sidebar.move @remote-threads
  Scenario: The sheet opens when something won't come with it
    When I ask to move the thread "Morning brief" to "bruce"
    Then the move sheet says "Move “Morning brief” to bruce"
    And the move sheet warns "1 background job, which gets stopped"
    And the move sheet offers the folder "/Users/bruce/dev/me/threadbear-threads/slot-4"
    And the move sheet's login is "claude-main"

  # The sheet asks the other runner as it's made, inside the app's own
  # update, and it read the app back for the runner's connection, which
  # crashed the GPUI app (a double lease, 12 Nov).
  @sidebar.move @remote-threads
  Scenario: Move to while this Mac's runner is away opens the sheet to wait
    When the runner goes away without a word for 8 seconds
    And the app has lost the runner
    And I ask to move the thread "Morning brief" to "bruce"
    Then the move sheet says "Move “Morning brief” to bruce"

  @sidebar.move @remote-threads
  Scenario: Moving from the sheet takes it off, and says so on the row
    When I ask to move the thread "Morning brief" to "bruce"
    Then the move sheet says "Move “Morning brief” to bruce"
    When I press Move in the move sheet for "bruce"
    Then the move sheet is closed
    And the row "Morning brief" says "Moving to bruce" on the right

  @sidebar.move @remote-threads
  Scenario: With nothing to decide, it goes straight away
    Given a move would leave nothing behind
    When I ask to move the thread "Morning brief" to "bruce"
    Then the row "Morning brief" says "Moving to bruce" on the right
    And the move sheet stayed closed

  @sidebar.move @remote-threads
  Scenario: A moving row offers Cancel Move instead
    Given a move would leave nothing behind
    When I ask to move the thread "Morning brief" to "bruce"
    Then the row "Morning brief" says "Moving to bruce" on the right
    When I right-click the row "Morning brief"
    Then the row's menu offers "Cancel Move"

  # Anything typed or answered while it moves could land on the wrong side
  # of the move, so the box and its card wait (ComposerView's `moving`). The
  # Swift app's dump, frozen since 12 Nov, has no "moving".
  @sidebar.move @remote-threads @swift-pending
  Scenario: The box is locked while its thread moves, and given back when the move is called off
    Given a move would leave nothing behind
    When I open the thread "Morning brief"
    And I ask to move the thread "Morning brief" to "bruce"
    Then the box is locked, saying "Moving to bruce once this turn ends…"
    When I cancel the move of "Morning brief"
    Then the box isn't locked

  @composer.runner @remote-threads
  Scenario: The star sets where new threads start
    When I start a new thread
    And I make "bruce" the default runner
    Then the "runner" menu has a row "[x] bruce *"
    And the composer is on the runner "bruce"
    When I make "This Mac" the default runner
    Then the "runner" menu has a row "[x] This Mac *"

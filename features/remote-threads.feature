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

  @sidebar.move @remote-threads
  Scenario: An old index row cannot undo a move to another Mac
    When I open the thread "Morning brief"
    And its runner lets the thread go to bruce while the old Mac still lists it
    Then the box is locked, saying "Moving to bruce…"
    And the row "Morning brief" isn't on another runner
    When the destination lists the moved thread
    Then the row "Morning brief" is on the runner "On bruce"
    And the composer is on the runner "bruce"

  @sidebar.move @remote-threads
  Scenario: A moved open thread resumes when its destination lists it
    When I open the thread "Morning brief"
    And its runner lets the thread go to bruce
    Then the box is locked, saying "Moving to bruce…"
    When the destination lists the moved thread
    Then the row "Morning brief" is on the runner "On bruce"
    And the box isn't locked
    When a reply arrives on the destination
    Then the transcript shows a reply starting "The destination is serving this thread"

  @sidebar.move @remote-threads
  Scenario: A thread moved between two other Macs keeps its destination
    When I open the thread "Tidy the media drive"
    And its runner lets the thread go to "cloud"
    Then the box is locked, saying "Moving to cloud…"
    And the row "Tidy the media drive" is on the runner "On bruce"
    When the destination lists the moved thread
    Then the row "Tidy the media drive" is on the runner "On cloud"
    When a reply arrives on the destination
    Then the transcript shows a reply starting "The destination is serving this thread"

  @sidebar.move @remote-threads
  Scenario: A thread moved back to this Mac resumes here
    When I open the thread "Tidy the media drive"
    And its runner lets the thread go to "This Mac"
    Then the box is locked, saying "Moving to This Mac…"
    And the row "Tidy the media drive" is on the runner "On bruce"
    When the destination lists the moved thread
    Then the row "Tidy the media drive" isn't on another runner
    When a reply arrives on the destination
    Then the transcript shows a reply starting "The destination is serving this thread"
    And the row "Tidy the media drive" isn't on another runner

  @sidebar.move @remote-threads
  Scenario: A move back to this Mac arrives after its source has cleared the move label
    When I open the thread "Tidy the media drive"
    And its move to this Mac is claimed before the local thread arrives
    Then the row "Tidy the media drive" isn't on another runner
    When a reply arrives on the destination
    Then the transcript shows a reply starting "The destination is serving this thread"

  @sidebar.move @remote-threads
  Scenario: Restarting the index during a waiting move keeps the source until it really moves
    When I open the thread "Tidy the media drive"
    And the index restarts while its move to this Mac is waiting
    Then the row "Tidy the media drive" is on the runner "On bruce"
    When that waiting move is cancelled
    And a reply arrives on the destination
    Then the transcript shows a reply starting "The destination is serving this thread"
    And the row "Tidy the media drive" is on the runner "On bruce"

  # Daybreak 113: a secondary runner taking its row out and putting it back
  # while a move here waits made the thread look like this Mac's, its box
  # unlocked, while every message still went to that runner.
  @sidebar.move @remote-threads
  Scenario: A thread on its way here stays its old runner's, locked, until this Mac has it
    When I open the thread "Tidy the media drive"
    And its runner lets the thread go to "This Mac"
    Then the box is locked, saying "Moving to This Mac…"
    And the row "Tidy the media drive" is on the runner "On bruce"
    When its old runner lists the thread again
    Then the box is locked, saying "Moving to This Mac…"
    And the row "Tidy the media drive" is on the runner "On bruce"
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

  # There's no folder picker for another Mac, so Other… asks for its path
  # (`typeRemoteFolder`). The Swift app has it under "+" › Folder, and its
  # automation, frozen since 12 Nov, can't open that dialog.
  @composer.runner @remote-threads @swift-pending
  Scenario: A new thread on another machine can start in a folder typed in
    When I start a new thread
    And I pick "bruce" from the "runner" menu
    Then the "runner" menu has a row "    [ ] Other…"
    When I type "/Users/bruce/dev/backup" as the folder on that machine
    Then the note under the box says "Starts in backup on bruce, with mike"
    And the "runner" menu has a row "    [x] /Users/bruce/dev/backup"

  @composer.runner @remote-threads
  Scenario: A new thread started on another machine is listed as its
    When I start a new thread
    And I pick "bruce" from the "runner" menu
    And I send "Check the backup drive's health"
    Then a thread called "Check the backup drive's health" opens
    And the transcript shows my message "Check the backup drive's health"
    And the row "Check the backup drive's health" is on the runner "On bruce"

  @sidebar.move @remote-threads
  Scenario: A moved thread stays in the sidebar before its destination lists it
    When I open the thread "Morning brief"
    And its runner lets the thread go to bruce
    Then the box is locked, saying "Moving to bruce…"
    And the row "Morning brief" isn't on another runner

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
  @sidebar.move @remote-threads
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

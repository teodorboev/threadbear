Feature: Swiping a thread's row away
  As someone clearing the Inbox
  I want to swipe a thread's row to archive it, as in Mail
  So putting a thread away is one movement

  Background:
    Given the app is connected to a runner with the demo threads

  # Made-up trackpad scrolls, through each app's own handling of them. The
  # trackpad itself is tried through the window server on Tinker Desk's
  # desktop (features/tools/real-gestures.ts) before a change to these lands.
  @sidebar.swipe
  Scenario: Two fingers left past the line archive the thread
    When I swipe the row "Morning brief" 200 points left
    Then the swipe went past the line
    And the "Archived" section lists "Morning brief"
    And the Archived note names "Morning brief"

  @sidebar.swipe
  Scenario: Let go short of the line, the row goes back into place
    When I swipe the row "Morning brief" 40 points left
    Then the swipe stopped short of the line
    And the "Inbox" section lists "Morning brief"

  # Eight scrolls to a swipe, and the Swift app's made-up ones carry whole
  # points, so these distances split evenly.
  @sidebar.swipe
  Scenario: The row follows the fingers, and is past the line once it's far enough along
    When I start swiping the row "Morning brief" 48 points left
    Then the row is 48 points along, short of the line
    When I swipe it 152 points further
    Then the row is past the line
    When I lift my fingers off the row
    Then the "Archived" section lists "Morning brief"

  @sidebar.swipe
  Scenario: An archived thread swiped goes back to the Inbox
    Given the "archived" section is unfolded
    And the "inbox" section is folded away
    When I swipe the row "Write the Quick Ask README" 200 points left
    Then the "Archived" section doesn't list "Write the Quick Ask README"

  # The Swift row's mouse drag is a SwiftUI gesture, which only real mouse
  # events reach, and a thread dragged out of it is an AppKit drag session,
  # which follows the real pointer: neither can be scripted there.
  @sidebar.mousedrag @swift-pending
  Scenario: A row dragged left with the mouse past the line is archived
    When I drag the row "Morning brief" 200 points left with the mouse
    Then the "Archived" section lists "Morning brief"

  @sidebar.mousedrag @swift-pending
  Scenario: A row dragged left short of the line goes back, and its thread is open
    When I drag the row "Morning brief" 30 points left with the mouse
    Then the "Inbox" section lists "Morning brief"
    And the open thread is "Morning brief"

  @drag.threadout @swift-pending
  Scenario: A row dragged into the open thread goes into its message as the thread's chip
    When I open the thread "Make uploads resume after a crash"
    And I drag the row "Morning brief" onto the transcript
    Then the open thread is "Make uploads resume after a crash"
    And the composer's chips are "thread: Morning brief"

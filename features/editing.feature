Feature: Editing a message
  As someone who sent a message I'd put differently
  I want to change it and carry on from there
  So the thread goes the way I meant

  Background:
    Given the app is connected to a runner with the demo threads

  @transcript.edit @composer.editing
  Scenario: The pencil puts a message in the box, and sending it goes back to there
    When I open the thread "Morning brief"
    And I edit my first message
    Then the composer holds my first message to edit
    When I press Escape
    Then the composer isn't editing

  @transcript.edit @composer.editing
  Scenario: Sending an edit replaces the message and carries on from it
    When I open the thread "Morning brief"
    And I edit my first message
    And I replace what's in the composer with "Only the calendar, please"
    And I press Return
    Then the composer isn't editing
    And the transcript has my message "Only the calendar, please"

  # Daybreak finding 86 (11 Nov): with one approval waiting, Return in an
  # empty box allows it, and the GPUI app did that mid-edit too.
  @transcript.edit @composer.editing @transcript.approval
  Scenario: Return in an emptied edit allows nothing
    When I open the thread "Make uploads resume after a crash"
    Then an approval card asks to run "rm -rf .build DerivedData"
    When I edit my first message
    And I replace what's in the composer with ""
    And I press Return
    Then the composer is still editing my first message
    And the approval for "rm -rf .build DerivedData" is still waiting

  # Thread 3fdaaeec (12 Nov): Mike's answer to a daily brief's card, with a
  # message from another thread's agent in its turn after it, and editing it
  # looked gone. The pencil is a real click through the window here, as only
  # calling its handler can't tell a pencil that's drawn but doesn't answer.
  # The GPUI app drew it as a plain ring and dropped his words from the bubble.
  @transcript.edit @composer.editing
  Scenario: The pencil by an answer to a card, with another agent's message after it
    Given a thread where I answered a card and another thread's agent wrote in after, called "Yesterday's brief"
    When I open the thread "Yesterday's brief"
    Then my answer to "How did yesterday feel?" shows "Good" and "It was good. I got a lot done."
    When I click the pencil by my message "It was good"
    Then the composer holds that message to edit, with "It was good" in it
    When I replace what's in the composer with "It was a flat day"
    And I press Return
    Then the composer isn't editing
    And the transcript has my message "It was a flat day"

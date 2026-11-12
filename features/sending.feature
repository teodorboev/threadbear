Feature: Sending a message
  As someone talking to an agent
  I want what I type to go straight in and the reply to stream back
  So it feels like a conversation

  Background:
    Given the app is connected to a runner with the demo threads
    And I open the thread "Morning brief"

  @composer.return @composer.send
  Scenario: Return sends what I typed
    When I type "Can you check the backups?" in the composer
    And I press Return
    Then the composer is empty
    And the transcript shows my message "Can you check the backups?"
    And the reply streams in and the thread goes back to idle

  @composer.return
  Scenario: Shift-Return starts a new line instead of sending
    When I type "first line" in the composer
    And I press Shift-Return
    Then the composer holds a new line after "first line"
    And the transcript doesn't show my message "first line"

  @composer.listcontinue
  Scenario: Shift-Return carries a numbered list on
    When I type "1. first" in the composer
    And I press Shift-Return
    Then the composer says the lines "1. first\n2. "

  @composer.listcontinue
  Scenario: Shift-Return on an empty list item ends the list
    When I type "1. first" in the composer
    And I press Shift-Return
    And I press Shift-Return
    Then the composer says the lines "1. first\n"

  @composer.textbox
  Scenario: Pasting puts the words where the cursor is
    When I type "Hello " in the composer
    And I paste "there\nworld" into the composer
    Then the composer says the lines "Hello there\nworld"

  @composer.textbox
  Scenario: What I've typed stays with its thread
    When I type "half a thought" in the composer
    And I open the thread "Rename the Settings tabs"
    Then the composer is empty
    When I open the thread "Morning brief"
    Then the composer says "half a thought"

  @composer.send
  Scenario: Stop ends a turn that's running
    When I send "Please keep working on this for a while"
    And I stop the turn
    Then the thread isn't running any more

  @composer.picturechips
  Scenario: A pasted picture goes in at the cursor as a chip
    When I type "look at " in the composer
    And I paste the picture into the composer
    Then the composer says the lines "look at [Picture 1] "
    And the composer holds 1 picture

  @composer.picturechips
  Scenario: The pictures go with the message, each in its place among the words
    When I type "compare " in the composer
    And I paste the picture into the composer
    And I type "and " in the composer
    And I paste the picture into the composer
    And I type "please" in the composer
    And I press Return
    Then the transcript shows my message "compare [Picture 1] and [Picture 2] please"

  @composer.filedrop @composer.dropanywhere
  Scenario: Dropping a picture on the thread adds it to the composer
    When I drop the picture on the transcript
    Then the composer holds 1 picture

  @composer.chiphover
  Scenario: Hovering a chip shows its picture
    When I paste the picture into the composer
    And the composer holds 1 picture
    Then hovering chip 1 shows its picture

  @composer.chipcopy
  Scenario: Cutting a picture's chip and pasting it back brings the picture with it
    When I type "look at " in the composer
    And I paste the picture into the composer
    And the composer holds 1 picture
    And I cut everything in the composer
    Then the composer is empty
    When I paste back what I cut
    Then the composer says the lines "look at [Picture 1] "
    And the composer holds 1 picture
    And the composer's chips are "picture: Picture 1"

  @composer.chipcopy
  Scenario: A copied picture's chip goes with the message once pasted
    When I type "compare " in the composer
    And I paste the picture into the composer
    And the composer holds 1 picture
    And I copy everything in the composer
    And I clear the composer
    And I paste back what I copied
    And the composer holds 1 picture
    And I press Return
    Then the transcript shows my message "compare [Picture 1]"

  @composer.threadid
  Scenario: A thread's id pasted on its own becomes that thread's chip
    When I paste the id of the thread "Rename the Settings tabs" into the composer
    Then the composer's chips are "thread: Rename the Settings tabs"
    And the composer holds the link to the thread "Rename the Settings tabs"

  @composer.threadid
  Scenario: An id no thread has stays words
    When I paste "0cf430ac-f429-4b1c-9d2e-7a1b2c3d4e5f" into the composer
    Then the composer has no chips
    And the composer says "0cf430ac-f429-4b1c-9d2e-7a1b2c3d4e5f"

  @composer.cardmention
  Scenario: A Notion card's link pasted in becomes the card's chip
    When I paste "see [Fix the share button](https://www.notion.so/Fix-the-share-button-1a2b3c4d) first" into the composer
    Then the composer's chips are "card: Fix the share button"
    And the composer says "see [Fix the share button](https://www.notion.so/Fix-the-share-button-1a2b3c4d) first"

  # The Swift app keeps drafts only when it's Mike's own app (AppDelegate
  # starts its DraftStore), never in a scripted run, so this one is driven on
  # the GPUI app only.
  @composer.textbox @swift-pending
  Scenario: What's typed and not sent is still in the box after the app is quit and opened again
    When I type "half a thought about the backups" in the composer
    And I wait a moment
    And the app is quit and opened again
    And I open the thread "Morning brief"
    Then the composer says "half a thought about the backups"

  @transcript.pending
  Scenario: A message the runner holds behind a turn says it's queued until it lands
    Given the runner holds each message 4 seconds before its turn starts
    When I open the thread "Morning brief"
    And I send "Check the logs too"
    Then the transcript's last row reads "(queued) Check the logs too"
    And the reply streams in and the thread goes back to idle

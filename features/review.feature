Feature: Design reviews
  As someone an agent shows design options to (its show_options)
  I want them in a window of their own, big enough to compare
  So I can step through them, tick one or several, say why and send it

  # The demo thread's review has four options, each one picture: A, B and C to
  # pick from, C the agent's pick, and D for reference only.
  Background:
    Given the app is connected to a runner with the demo threads
    When I open the thread "Compare account picker designs"

  @review.window @transcript.review
  Scenario: The card opens the review on its first option, one at a time
    When I press "Compare 4 images"
    Then the review window is open on option "A", one at a time

  @review.window
  Scenario: A review: link in the agent's reply opens that review
    When I follow the link "review:review_account" in the agent's last reply
    Then the review window is open on option "A", one at a time

  # An ask's link is followed from inside the composer's own update, and the
  # transcript read the composer back to reach the app, which crashed the
  # GPUI app ("Open the design options" on a card, 12 Nov).
  @review.window @card.needsyou
  Scenario: A review: link on an ask in the composer opens that review
    When I send "Which one? Pick a design"
    Then the card in the composer asks "Pick one of the account picker designs"
    When I press link 1 of ask 1 on the card
    Then the review window is open on option "A", one at a time

  @header.pendingreviews
  Scenario: The bar over the thread says how many reviews are waiting
    Then the bar over the thread says "Review design options (1)"

  # The Swift bar's list is a popover, a window of its own that the Swift
  # app's `click` doesn't look in.
  @header.pendingreviews @swift-pending
  Scenario: The bar's list opens a review that's waiting
    When I press the bar's "Review design options (1)"
    And I press the review "Account picker" in the bar's list
    Then the review window is open on option "A", one at a time

  @review.picks
  Scenario: Ticking options says what Send will send
    When I open the review
    And I tick "B" in the review
    And I tick "C" in the review
    Then the review's Send says "Send B + C"
    When I choose None in the review
    Then the review's Send says "Send none of them"
    And no option is ticked in the review

  @review.picks
  Scenario: An option there for reference can't be ticked
    When I open the review
    Then "D" has no tick in the review

  @review.picks
  Scenario: Closing the window keeps the draft
    When I open the review
    And I tick "B" in the review
    And I write "Bigger, please" in the review
    And I close the review
    And I open the review
    Then the review has "B" ticked and the reply "Bigger, please"

  # Voice Type puts what Mike says on the clipboard and presses ⌘V in the
  # window in front. He ticks an option and talks straight away, before
  # clicking the reply box, and the words went nowhere (12 Nov). The Swift
  # app's `clipboard` can't be set from a script.
  @review.picks @swift-pending
  Scenario: Words pasted after ticking an option go in the reply box
    When I open the review
    And I tick "B" in the review
    And I paste "Bigger, please" in the review with ⌘V
    Then the review has "B" ticked and the reply "Bigger, please"
    When I paste ". That one." in the review with ⌘V
    Then the review has "B" ticked and the reply "Bigger, please. That one."
    When I close the review
    And I open the review
    Then the review has "B" ticked and the reply "Bigger, please. That one."

  @review.picks
  Scenario: Sending answers the agent, and the review can't be changed after
    When I open the review
    And I tick "C" in the review
    And I write "C, please." in the review
    And I press Send in the review
    Then the review window has closed
    And the transcript shows my message "Review: Account picker"
    When I press "Open review"
    Then the review window shows what was sent: "C" and "C, please."
    And nothing can be changed in the review

  @review.nav
  Scenario: Next steps through every option, then shows them all to choose from
    When I open the review
    And I press "Next (→)" in the review
    Then the review is on option "B", shot 1
    When I press "Next (→)" in the review
    And I press "Next (→)" in the review
    Then the review is on option "D", shot 1
    When I press "See them all and choose (→)" in the review
    Then the review shows them all
    When I press "Back to the last one (←)" in the review
    Then the review is on option "D", shot 1

  @review.nav
  Scenario: Every shot of an option comes before the next option
    Given the agent shows options with several shots each
    When I open that review
    Then the review is on option "A", shot 1
    When I press "Next (→)" in the review
    Then the review is on option "A", shot 2
    When I press "Next (→)" in the review
    And I press "Next (→)" in the review
    Then the review is on option "B", shot 1
    When I press "Previous (←)" in the review
    Then the review is on option "A", shot 3
    When I press "B. One picker, shot 2" in the review
    Then the review is on option "B", shot 2

  # The Swift scripts' keys go to the main window, not a review's.
  @review.nav @swift-pending
  Scenario: The arrow keys step through the shots, and Return shows them all
    Given the agent shows options with several shots each
    When I open that review
    And I press the "right" key in the review
    Then the review is on option "A", shot 2
    When I press the "down" key in the review
    Then the review is on option "B", shot 1
    When I press the "enter" key in the review
    Then the review shows them all
    When I press the "enter" key in the review
    Then the review is on option "B", shot 1

  @review.zoom @review.minimap
  Scenario: Full size shows the shot at its own size, with a minimap of all of it
    When I open the review
    And I show the shot at full size
    Then the shot is zoomed in, with the minimap showing
    When I fit the shot in its space
    Then the shot fits, with no minimap

  # The Swift scripts' keys go to the main window, not a review's.
  @review.zoom @swift-pending
  Scenario: Esc takes a zoomed shot back to fitting, then shows them all
    When I open the review
    And I show the shot at full size
    And I press the "escape" key in the review
    Then the shot fits, with no minimap
    When I press the "escape" key in the review
    Then the review shows them all

  @review.holdspace
  Scenario: Today's version can be flicked to and back
    When I open the review
    And I press "Flick to Moving mid-thread: click, or hold Space" in the review
    Then the review shows today's version over option "A"
    When I press "Back to A (or let go of Space)" in the review
    Then the review shows option "A" again

  @review.openlive
  Scenario: An option that runs live can be tried from its caption
    Given the agent shows options with several shots each
    When I open that review
    Then the caption offers nothing to try live
    When I press "C. Account pools" in the review
    Then the caption offers "http://localhost:5190/c" to try live

  @review.media @windows-pending @linux-pending
  Scenario: A clip plays on the stage
    Given the agent shows options with several shots each
    When I open that review
    And I press "C. Account pools" in the review
    Then the stage plays the clip

  # A page shows in place, sealed with its scripts off, as the pane's local
  # pages do: the GPUI window through the web runtime, which isn't on
  # Windows and Linux yet (there it's the still, with Open the page).
  @review.media @web-runtime @windows-pending @linux-pending
  Scenario: A page shows in place on the stage
    Given the web runtime is installed
    And the agent shows options with several shots each
    When I open that review
    And I press "B. One picker, shot 2" in the review
    Then the stage shows the page

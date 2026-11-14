Feature: A thread comes in as it opens
  As someone moving between threads all day
  I want the thread I pick to ease into place, and a new one to settle where it'll stay
  So switching feels smooth without ever feeling slower

  # The GPUI app's own (open_motion.rs), from mockups Mike picked on 13 Nov:
  # the Swift app swaps threads on the spot, and its dump doesn't say how a
  # thread opened.

  Background:
    Given the app is connected to a runner with the demo threads

  @juice.opening @swift-pending
  Scenario: A clicked thread rises into place
    When I click the row "Morning brief"
    Then the open thread eases in as clicked

  @juice.opening @swift-pending
  Scenario: The arrow keys bring the next thread in from the way they went
    When I click the row "Morning brief"
    And I press the down arrow
    Then the open thread eases in from below
    When I press the up arrow
    Then the open thread eases in from above

  @juice.opening @swift-pending
  Scenario: The thread after an archived one fades in under the flight, with no motion of its own
    Given I open the thread "Morning brief"
    When I archive the open thread
    Then another thread opens in its place
    And the open thread has no motion of its own

  @juice.opening @swift-pending
  Scenario: A new thread's box drops from the page to its place under the transcript
    When I start a new thread
    And I send "Check the landing copy"
    Then a thread called "Check the landing copy" opens
    And its box drops from the new-thread page to its place

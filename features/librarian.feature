Feature: The Librarian
  As someone whose threads pile up into projects
  I want to see what the librarian is doing, what it will look at and what it did
  So I can trust it to keep my projects tidy, and stop it when I need to

  Background:
    Given the app is connected to a runner with the demo threads

  @librarian.log
  Scenario: The view says what it's doing, what it'll look at next and what it did
    When I click "Librarian" in the activity bar
    Then the sidebar shows the "librarian" view
    And the librarian says "Looking at “Rebuild the three landing pages”"
    And the "Next" section lists "Make a poster for the meetup"
    And the "Recent" section lists "Put it in Threadbear"
    And the activity bar's "Librarian" button counts 4

  @librarian.page
  Scenario: The page beside it counts today's looks
    When I click "Librarian" in the activity bar
    Then the Librarian page is in place of the thread
    And the page's tiles include "14 Looks today, 3 Skipped today, 52k Tokens today, 3 Waiting"

  @librarian.log
  Scenario: A project in the recent log opens its page
    When I click "Librarian" in the activity bar
    And I follow a project in the librarian log
    Then the librarian's project page is open

  @librarian.log
  Scenario: Pause shows at once, and Resume carries on
    When I click "Librarian" in the activity bar
    And I press the row button "Stop looking at threads"
    Then the librarian says "Paused"
    When I press the row button "Let it look at threads again"
    Then the librarian says "Looking at “Rebuild the three landing pages”"

  # Swift's scripted form switch waits for an actual mouse-up.
  @librarian.page @swift-pending
  Scenario: The page switch pauses and resumes the librarian
    When I click "Librarian" in the activity bar
    And I scroll to the Librarian settings
    And I press the row button "Turn it off to pause; what's queued waits"
    Then the librarian says "Paused"
    When I press the row button "Paused: what's queued waits. Turn it on to carry on"
    Then the librarian says "Looking at “Rebuild the three landing pages”"

@highlights.board @swift-pending
Feature: The Notion board beside a thread
  Background:
    Given the app is connected to a runner with the demo threads
    When I open the thread "Pick a colour for the new Share button"

  @highlights.board
  Scenario: Mentioning a card twice keeps the draft and its picture
    When I prepare a board question with a picture
    And I press the board card "Pick the share button colour"
    And I press the board card "Pick the share button colour"
    Then the board card "Pick the share button colour" is mentioned once after my words
    And the board question still has its picture

  @highlights.board
  Scenario: Done cards fold away and can be opened again
    Then the board card "Battery check on the old Pixel" is hidden
    When I unfold the board's Done column
    Then the board card "Battery check on the old Pixel" can be seen
    When I fold the board's Done column
    Then the board card "Battery check on the old Pixel" is hidden

  @highlights.board
  Scenario: A card opens the other thread working on it
    When I open the working thread from the board card "Dark mode for the dashboard"
    Then the board has opened the thread "demo_dark_mode"

  @highlights.board
  Scenario: Long columns show four cards until More is pressed
    When the board gains five more cards
    Then the board card "Additional board card 5" is hidden

    When I show all cards in the board's To do column
    Then the board card "Additional board card 5" can be seen
    When I show fewer cards in the board's To do column
    Then the board card "Additional board card 5" is hidden

  @real-screen @linux-pending @windows-pending @highlights.board
  Scenario: Trackpad gestures belong to the board rather than page navigation
    When I check the board with real two-finger gestures

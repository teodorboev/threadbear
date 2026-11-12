Feature: Swiping back and forward
  As someone moving between threads
  I want two fingers across the window to go back and forward, as in Safari
  So I can step through where I've been without reaching for the keys

  Background:
    Given the app is connected to a runner with the demo threads
    And I open the thread "Morning brief"
    And I open the thread "Make uploads resume after a crash"

  # Made-up trackpad scrolls across the middle of the window, the content's
  # way, through each app's own handling of them. The trackpad itself is
  # tried through the window server on Tinker Desk's desktop
  # (features/tools/real-gestures.ts) before a change to these lands.
  @nav.swipe
  Scenario: Two fingers to the right past the line go back
    When I swipe the window 300 points right
    Then the page swipe went past the line
    And the open thread is "Morning brief"

  @nav.swipe
  Scenario: And to the left go forward again
    When I swipe the window 300 points right
    And I swipe the window 300 points left
    Then the open thread is "Make uploads resume after a crash"

  @nav.swipe
  Scenario: Part way along, a card says where letting go will go
    When I start swiping the window 100 points right
    Then the card in the window says "Back to | Morning brief", short of the line
    When I swipe the window 200 points further right
    Then the card in the window is past the line
    When I lift my fingers off the window
    Then the open thread is "Morning brief"
    And there's no card in the window

  @nav.swipe
  Scenario: Let go short of the line, it stays where it is
    When I swipe the window 80 points right
    Then the page swipe stopped short of the line
    And the open thread is "Make uploads resume after a crash"

  @nav.swipe
  Scenario: With nowhere to go forward, the card says so and nothing happens
    When I start swiping the window 120 points left
    Then the card in the window says "Nothing to go forward to"
    When I lift my fingers off the window
    Then the open thread is "Make uploads resume after a crash"

  @nav.swipe
  Scenario: Up and down is a scroll, not a swipe
    When I swipe the window 300 points down
    Then the page swipe never started
    And the open thread is "Make uploads resume after a crash"

Feature: Panels glide open and closed
  As someone opening and putting away panels beside the thread
  I want the panels to make room and take it back smoothly
  So I can see where a panel came from and where its room went

  Background:
    Given the app is connected to a runner with the demo threads
    And the thread "Rename the Settings tabs" works in a folder of its own
    And I open the thread "Rename the Settings tabs"

  @dock.motion
  Scenario: Bringing up the Terminal, the thread's panel makes room for it
    When I press ⌘J
    Then the panels glide, with "thread" smaller than it was
    And "terminal" glides in where it wasn't

  @dock.motion
  Scenario: Putting the Terminal away, the thread's panel takes its room back
    Given I press ⌘J
    And the panels have settled with "terminal" in them
    When I press ⌘J
    Then the panels glide, with "thread" bigger than it was
    And "terminal" glides out

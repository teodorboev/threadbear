Feature: Settings › Runners
  As someone whose threads run on more than one machine
  I want my machines listed in my order, and to choose where new threads start
  So new threads go where I want them, from this Mac and from my phone

  Background:
    Given the app is connected to a runner with the demo threads

  @settings.runners
  Scenario: The runners are in my order, with a star where new threads start
    When I open Settings on "runners"
    Then Settings lists the runners "m5 *: This Mac" and "bruce: Online"
    And Settings says new threads start on "This Mac"

  @settings.runners
  Scenario: Picking another machine for new threads moves the star
    When I open Settings on "runners"
    And I pick "bruce" for new threads in Settings
    Then Settings says new threads start on "bruce"
    And Settings lists the runners "m5: This Mac" and "bruce *: Online"
    When I pick "This Mac" for new threads in Settings
    Then Settings lists the runners "m5 *: This Mac" and "bruce: Online"

  # The Swift list moves by a real drag, which its scripts can't make without
  # the real pointer (AGENTS.md, "Tests that need the real screen").
  @settings.runners @swift-pending
  Scenario: Dragging a runner changes the order, here and in the Runners panel
    When I open Settings on "runners"
    And I drag "bruce" onto "m5" in Settings
    Then Settings lists the runners "bruce: Online" and "m5 *: This Mac"
    When I open the Runners panel
    Then the Runners panel lists "bruce" before "m5"

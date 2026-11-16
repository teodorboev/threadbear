Feature: Settings › Runners
  As someone whose threads run on more than one machine
  I want my machines listed in my order, and to choose where new threads start
  So new threads go where I want them, from this Mac and from my phone

  Background:
    Given the app is connected to a runner with the demo threads

  @settings.runners
  Scenario: The runners are in my order, with a star where new threads start
    When I open Settings on "runners"
    Then Settings lists the runners "laptop *: This Mac" and "mac-mini: Online"
    And Settings says new threads start on "This Mac"

  @settings.runners
  Scenario: Picking another machine for new threads moves the star
    When I open Settings on "runners"
    And I pick "mac-mini" for new threads in Settings
    Then Settings says new threads start on "mac-mini"
    And Settings lists the runners "laptop: This Mac" and "mac-mini *: Online"
    When I pick "This Mac" for new threads in Settings
    Then Settings lists the runners "laptop *: This Mac" and "mac-mini: Online"

  # The Swift list moves by a real drag, which its scripts can't make without
  # the real pointer (AGENTS.md, "Tests that need the real screen").
  @settings.runners @swift-pending
  Scenario: Dragging a runner changes the order, here and in the Runners panel
    When I open Settings on "runners"
    And I drag "mac-mini" onto "laptop" in Settings
    Then Settings lists the runners "mac-mini: Online" and "laptop *: This Mac"
    When I open the Runners panel
    Then the Runners panel lists "mac-mini" before "laptop"

Feature: Settings
  As someone who wants Threadbear set up my way
  I want the runner's settings in one window, each saying what it's for
  So I can change the few that matter here, and ask an agent for the rest

  Background:
    Given the app is connected to a runner with the demo threads

  @settings.window
  Scenario: Settings opens on General, and another section can be picked
    When I open Settings
    Then Settings shows "general"
    When I open Settings on "models"
    Then Settings shows "models"

  # A scripted Swift app's arrow keys go only to a text box (`arrow`), so the
  # list's arrows are checked on the GPUI app.
  @settings.window @swift-pending
  Scenario: Up and down move through Settings' sections
    When I open Settings
    And I press the down arrow in Settings
    Then Settings shows "models"
    When I press the down arrow in Settings
    Then Settings shows "pools"
    When I press the up arrow in Settings
    Then Settings shows "models"

  # The Swift app's Settings is an AppKit form: a scripted click on one of its
  # controls starts a tracking loop that waits for a real mouse-up, so the
  # command never answers. Those scenarios are driven on the GPUI app only.
  @settings.general @swift-pending
  Scenario: Turning off Keep the Mac awake changes the runner's setting
    When I open Settings
    Then Settings says the Mac is kept awake
    When I press "Stop your Mac sleeping on its own while a thread is working" in Settings
    Then Settings says the Mac isn't kept awake

  @settings.models @swift-pending
  Scenario: A private folder can be taken off
    When I open Settings on "models"
    Then Settings lists the private folder "~/dev/clients/acme"
    When I press "Stop keeping this folder private" in Settings
    Then Settings lists no private folders

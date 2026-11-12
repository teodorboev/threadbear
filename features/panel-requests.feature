Feature: An agent shows Mike a panel
  As someone an agent wants to show something
  I want the panel it names to come up when I look at its thread
  So I see what it means without hunting for it

  Background:
    Given the app is connected to a runner with the demo threads

  @panel.requests
  Scenario: show_panel brings the panel up once Mike looks, and tells the runner
    When I open the thread "Morning brief"
    And I'm looking at the app
    And the agent asks to show the "updates" panel
    Then the dock has the "updates" panel
    And the runner heard the panel was shown

  # A pin shown with `show: true` opens in the pane once Mike looks. The pane
  # was told while the app was mid-update and read the app back, which
  # crashed the GPUI app on opening such a thread (12 Nov).
  @panel.requests @highlights.pinned
  Scenario: A pin the agent showed opens in the pane once Mike looks
    Given the agent of "Morning brief" pinned a file of its own and showed it
    When I open the thread "Morning brief"
    And I'm looking at the app
    Then the pane is showing the pinned file

  @panel.requests
  Scenario: A panel the thread hasn't got is left waiting
    When I open the thread "Morning brief"
    And I'm looking at the app
    And the agent asks to show the "nonesuch" panel
    And I wait a moment
    Then the runner still has the request waiting

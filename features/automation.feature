Feature: Driving a private preview of the app
  An agent can prepare screenshots without exposing login names
  and inspect a live reply without changing the thread.

  Background:
    Given the app is connected to a runner with the demo threads

  @system.testharness
  Scenario: A screenshot can use login ids and a chosen amount of room
    When I prepare anonymous logins with 44 percent room
    And I click the honey pot in the activity bar
    Then the screenshot logins have anonymous names and 44 percent room

  @system.testharness
  Scenario: The script can read live text and a row's debug information
    When I open the thread "Rebuild the three landing pages"
    Then the script can read the streaming text
    And the script can copy a row's debug information

  @system.testharness
  Scenario: Older scripts can size a window and click a control by coordinates
    When I size the preview window to 1000 by 600
    Then the script reports a window id and size 1000 by 600
    When I click Librarian by its coordinates
    Then the sidebar shows the "librarian" view

  @system.testharness
  Scenario: A script selects a deployment the Convex panel offers
    When I open the thread "Add dark mode to the dashboard"
    And I give that thread a Convex panel
    When I select the production deployment through the script
    Then the script reports the production Convex dashboard
    And the script refuses an unknown deployment

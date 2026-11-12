Feature: Bringing the Swift app's settings across
  As someone moving from the Swift app to this one
  I want my settings and unsent drafts there the first time I open it
  So the new app starts out the way I had the old one

  Background:
    Given the app is connected to a runner with the demo threads

  # The Swift app is where these come from, so it has nothing to bring.
  @system.swiftimport @swift-pending
  Scenario: The first launch brings the zoom, the Inbox's order and a draft across, once, and says so
    Given the Swift app had its zoom at 125%, the Inbox newest first, and "half a thought" in the box of "Morning brief"
    When the app opens again
    Then the app's zoom is 125%
    And the Inbox is sorted newest first
    And a note says "Brought across from the Swift app: your zoom and Inbox order, and an unsent draft."
    When I open the thread "Morning brief"
    Then the composer says "half a thought"
    When the app opens again
    Then no note says what came across from the Swift app

Feature: The activity log
  As someone whose agents change Threadbear for me
  I want the app to note what I click, open and send, once I've switched it on
  So they can see how I really use it, while nothing I type ever goes in it

  Background:
    Given the app is connected to a runner with the demo threads

  # Both apps write the same lines to the same files, so `tb activity` reads
  # them as one. The press only asks the log what it would note: nothing is
  # pressed.
  @system.activitylog
  Scenario: A click is noted by the tooltip of what was clicked, and where it was
    Given the activity log is on
    When I click the control with the tip "Sort: Needs you first", for the activity log
    Then the activity log has a click on "Sort: Needs you first" in the threads panel

  @system.activitylog
  Scenario: A message sent is noted by what came with it, never its words
    Given the activity log is on
    When I open the thread "Morning brief"
    And I send "nothing of these words goes in the log"
    Then the activity log has a message sent in "Morning brief"
    And the activity log has nothing of "nothing of these words goes in the log"

  # A window coming to the front is heard inside the app's own update, and
  # naming it there read the app again: opening Settings with the log on
  # crashed the GPUI app (12 Nov). The Swift app's scripted windows can't be
  # brought to the front this way.
  @system.activitylog @swift-pending
  Scenario: Settings coming to the front is noted by its window
    Given the activity log is on
    When the Settings window comes to the front, for the activity log
    Then the activity log has the window "settings"

  @system.activitylog
  Scenario: Off, it notes nothing
    Given the activity log is off
    When I click the control with the tip "Sort: Needs you first", for the activity log
    Then the activity log has nothing from today

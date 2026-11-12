Feature: Scheduled messages
  As someone who leaves messages to send themselves later
  I want every schedule in one list, with what's coming up and what went wrong
  So I can see at a glance what will happen, and change it there

  Background:
    Given the app is connected to a runner with the demo threads

  @schedule.list
  Scenario: Schedules fall under Active, Paused and Done
    When I click "Scheduled" in the activity bar
    Then the sidebar shows the "scheduled" view
    And the "Active" section lists "Check the CI run again"
    And the "Paused" section lists "Nightly backup report"
    And the "Done" section lists "Check the domain renewal went through"
    And the "Done" section is folded

  @schedule.list
  Scenario: A row says when it runs next, where it goes, and what went wrong last time
    When I click "Scheduled" in the activity bar
    Then the schedule "Check the CI run again" runs "in 40m"
    And the schedule "Check the CI run again" sends to "Why is the build slow on CI?"
    And the schedule "Photo library check" runs on "bruce" and sends to "New thread in Photos"
    And the schedule "Disk space report" notes "Waiting for bruce to run it"
    And the schedule "Weekly dependency check" notes "Skipped last run: the last thread it started was still running"
    And the schedule "Nightly backup report" runs "Paused"
    And the schedule "Nightly backup report" notes "Last run failed: the folder /Users/mike/dev/old-backups is gone"

  @schedule.page
  Scenario: The page beside the list says what's next and counts them
    When I click "Scheduled" in the activity bar
    Then the Scheduled page is in place of the thread
    And the page says what's next, starting "Next: "
    And the page's tiles include "6 On, 5 Repeating, 1 Set by a thread, 1 Paused, 49 Runs so far"

  @schedule.page
  Scenario: Picking a thread puts it back in place of the page, and the list stays
    Given the thread "Morning brief" is in the inbox
    When I click "Scheduled" in the activity bar
    And I pick the thread "Morning brief" from where I am
    Then no page is in place of the thread
    And the sidebar shows the "scheduled" view

  # Back to a page put the thread behind it back first and recorded that
  # as a place of its own, which cut off Forward (Codex's review, 11 Nov).
  @schedule.page @nav.history
  Scenario: Back from a thread picked beside the page brings the page back, and Forward the thread
    Given the thread "Rename the Settings tabs" is in the inbox
    When I open the thread "Morning brief"
    And I click "Scheduled" in the activity bar
    And I pick the thread "Rename the Settings tabs" from where I am
    Then no page is in place of the thread
    When I press the mouse's "back" button
    Then the Scheduled page is in place of the thread
    When I press the mouse's "forward" button
    Then no page is in place of the thread
    And the open thread is "Rename the Settings tabs"

  @schedule.list
  Scenario: Pausing a schedule moves it under Paused at once, and Resume brings it back
    When I click "Scheduled" in the activity bar
    And I hover the row "Weekly dependency check"
    Then the row "Weekly dependency check" offers the buttons "Run now, Pause, Delete"
    When I press the row button "Pause"
    Then the "Paused" section lists "Weekly dependency check"
    When I hover the row "Weekly dependency check"
    And I press the row button "Resume"
    Then the "Active" section lists "Weekly dependency check"

  @schedule.list
  Scenario: Another Mac's schedule is asked of that Mac
    When I click "Scheduled" in the activity bar
    And I hover the row "Photo library check"
    Then the row "Photo library check" offers the buttons "Ask bruce to run it now, Pause, Delete"

  @schedule.editor
  Scenario: New schedule opens the editor on weekdays at 9
    When I click "Scheduled" in the activity bar
    And I press the row button "Schedule a message for later"
    Then the schedule editor is open on a new schedule that repeats on "0 9 * * 1-5"

  @schedule.editor
  Scenario: A schedule opens in the editor with what it sends and when
    When I click "Scheduled" in the activity bar
    And I open the schedule "Weekly dependency check" in the editor
    Then the schedule editor is open on "Look for outdated npm and Swift packages in ~/dev/me and list the ones worth updating." repeating on "0 10 * * 1"

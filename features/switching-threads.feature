Feature: Switching threads
  As someone moving between threads all day
  I want a thread to show at once when I come back to it
  So I'm never left looking at an empty transcript while the runner's busy

  # The mock runner takes 3 seconds over every read of a thread here, as a
  # runner handing over to a new release or a loaded Mac can.
  Background:
    Given the app is connected to a runner with the demo threads

  @thread.loading
  Scenario: A thread being read shows a spinner where its transcript will be
    When I start opening the thread "Make uploads resume after a crash"
    Then the transcript is a spinner with no rows
    And the transcript shows my message "Uploads start again from zero if the app quits"

  @thread.kept
  Scenario: Going back to a thread shows it straight away
    When I open the thread "Make uploads resume after a crash"
    And I open the thread "Make a poster for the meetup"
    And I start opening the thread "Make uploads resume after a crash"
    Then straight away the transcript shows my message "Uploads start again from zero if the app quits"

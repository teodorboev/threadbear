Feature: The tour, the pages' intros and Finish setting up
  As someone opening Threadbear for the first time
  I want a short look round the window, with something to try things on
  So I know where things are before I send my first thread

  Background:
    Given the app is connected to a runner with the demo threads

  # The whole tour, sent thread and all, is played against a real dev runner
  # by app/Tools/tour-e2e.mjs (Swift) and desktop/scripts/tour-e2e.mjs (GPUI):
  # the mock fakes samples and trust. These are the parts a mock shows true.
  @tour.firstrun
  Scenario: Help's tour starts with a welcome, makes samples and lights each part in turn
    When I choose "Take the Tour" from the menu bar
    Then the tour shows "Welcome to Threadbear"
    And the sample threads "Fix the flaky login test", "Plan the weekend" and "Draft the release notes" are listed
    When I go on in the tour
    Then the tour shows "Threads" with a part of the window lit
    When I go on in the tour
    Then the tour shows "Star or put away" with a part of the window lit
    And "Plan the weekend" is drawn hovered, with its buttons out

  @tour.firstrun
  Scenario: Ending the tour takes its samples away
    When I choose "Take the Tour" from the menu bar
    And the sample threads "Fix the flaky login test", "Plan the weekend" and "Draft the release notes" are listed
    And I end the tour
    Then the tour is gone
    And no sample thread is listed

  @tour.intros
  Scenario: A page explains itself with one card the first time it opens, and only then
    Given the pages' intros are on
    When I switch to the Projects view
    Then the tour shows "Projects" as a single card
    When I go on in the tour
    And I switch to the Inbox view
    And I switch to the Projects view
    Then no tour card shows

  # The Swift dump has no `setup` and its app is frozen.
  @tour.setup @swift-pending
  Scenario: Help > Finish Setting Up opens the list, ticked off from what the app knows
    When I choose "Finish Setting Up" from the menu bar
    # The mock's threads have run and it has a phone and a browser paired.
    Then Finish setting up is open with "Start your first thread", "Pair your iPhone" and "Open your website" all done

  # How a launch decides on the tour by itself. The Swift app keeps whether
  # it's been seen in its own defaults, and a scripted Swift app never starts
  # it, so these are the GPUI app's: it reads the Swift app's on first launch.
  @tour.firstrun @swift-pending
  Scenario: A new install gets the tour by itself, with no Skip
    Given the app opens as a new install would, with no Swift app before it
    Then the tour shows "Welcome to Threadbear"
    And the tour can't be skipped

  @tour.firstrun @swift-pending
  Scenario: Someone who finished the Swift app's tour doesn't get it again
    Given the app opens as a new install would, after a Swift app whose tour was finished
    Then no tour card shows

  @tour.firstrun @swift-pending
  Scenario: Someone who never finished the Swift app's tour gets it
    Given the app opens as a new install would, after a Swift app whose tour was never finished
    Then the tour shows "Welcome to Threadbear"

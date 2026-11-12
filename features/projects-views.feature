Feature: Projects: the list, the page of cards and a project's own page
  As someone whose threads are grouped into projects
  I want to see each project's threads, where it's up to and what's in it
  So I can find my way round a body of work, not just one thread at a time

  Background:
    Given the app is connected to a runner with the demo threads

  @projects.home
  Scenario: Each project's card says what its threads are doing
    When I show the Projects page
    Then the project card "Photo sync" says "1 needs you · 1 working"
    And the project card "Threadbear" says "1 working · 1 new"

  @projects.home
  Scenario: The search finds a project by what it's about
    When I show the Projects page
    And I search the projects for "chunk size"
    Then the Projects page shows the cards "Photo sync"
    When I search the projects for "walk"
    Then the Projects page shows the cards "Walkies"

  @projects.list
  Scenario: The sidebar lists each project's threads under it, and the rest after
    When I show the Projects page
    Then the sidebar's project "Threadbear" lists "Rename the Settings tabs"
    And the sidebar's project "Photo sync" lists "Why is the build slow on CI?"
    And the sidebar's project "Threadbear" lists "Why is the build slow on CI?"
    And the sidebar's project "Not in a project" lists "Ship the beta to TestFlight"

  @projects.page
  Scenario: A project's page has its folders and threads
    When I open the project "Walkies"
    Then the project page lists the folders "/Users/mike/dev/walkies, /Users/mike/dev/walkies-backend"
    And the project page lists 2 threads

  @header.projectchip @projects.page
  Scenario: A thread's project tile opens the project's page, and the thread comes back
    When I open the thread "Make uploads resume after a crash"
    And I click the project tile "Photo sync"
    Then the project page "Photo sync" is open
    When I open the thread "Make uploads resume after a crash"
    Then no project page is open

  @nav.history @projects.page
  Scenario: Back from a project's page goes back to the thread
    When I open the thread "Make uploads resume after a crash"
    And I click the project tile "Photo sync"
    Then the project page "Photo sync" is open
    When I press the mouse's "back" button
    Then no project page is open

  @projects.newthread
  Scenario: New Thread on a project's page starts a thread in it
    When I open the project "Walkies"
    And I press New Thread on the project's page
    Then the new-thread page names the project "In Walkies"

  @sidebar.row.menu
  Scenario: A thread's menu has its projects
    When I right-click the row "Ship the beta to TestFlight"
    Then the row's menu offers "Projects"

  # The ⓘ's popover is a window of its own in the Swift app, which its
  # scripted click doesn't look in.
  @highlights.projects @swift-pending
  Scenario: Open project in a thread's Highlights opens the project's page
    When I open the thread "Add dark mode to the dashboard"
    And I ask why the thread is in the project "Walkies"
    And I press the control "Open Walkies's page"
    Then the project page "Walkies" is open

  @panel.projectpage
  Scenario: A project's page has the panels its folders give, and no Highlights
    When I open the project "Walkies"
    Then the right rail has a "Patches" button
    And the right rail has no "Highlights" button

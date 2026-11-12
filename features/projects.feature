Feature: Decisions on the Projects page
  As someone with several threads waiting on me in one project
  I want their questions in one place, answered one after another
  So I don't open each thread to answer it, and nothing is relayed

  Background:
    Given the app is connected to a runner with the demo threads

  @projects.decisions @project-decisions
  Scenario: A project's card says how many decisions are waiting
    When I show the Projects page
    Then the project card "Threadbear" says "4 decisions"

  @projects.decisions @project-decisions
  Scenario: A project's page opens on its threads' questions
    When I open the project "Threadbear"
    Then the project's decisions include "Rename the Settings tabs" asking "Call the Advanced tab \"Machine\" or \"Runner\"?"
    And the project's decisions include "Draft the release notes for 0.4" asking "Lead with projects or with the librarian?"

  @projects.decisions @project-decisions
  Scenario: Answering on the page answers the thread
    When I open the project "Threadbear"
    And I pick answer 1 of ask 1 on the decision from "Compare SQLite and plain files for the runner"
    Then the project's decisions don't include "Compare SQLite and plain files for the runner"
    When I open the thread "Compare SQLite and plain files for the runner"
    Then the transcript shows my message "Keep plain files for now?"

  # The answer goes from inside the card's own update, and with the runner
  # away its picks were put back by updating the card again, which crashed
  # the GPUI app (a double lease, 12 Nov).
  @projects.decisions @project-decisions
  Scenario: Answering while the runner's away says so and keeps the question
    When I open the project "Threadbear"
    And the runner goes away without a word for 8 seconds
    And the app has lost the runner
    And I pick answer 1 of ask 1 on the decision from "Compare SQLite and plain files for the runner"
    Then the error bar says "Not connected to the runner, so that wasn't sent."
    And the project's decisions include "Compare SQLite and plain files for the runner" asking "Keep plain files for now?"

  @projects.decisions @project-decisions
  Scenario: A card of several questions waits until each has an answer
    When I open the project "Threadbear"
    And I pick answer 1 of ask 1 on the decision from "Draft the release notes for 0.4"
    Then the project's decisions include "Draft the release notes for 0.4" asking "Mention the Windows build?"
    When I pick answer 2 of ask 2 on the decision from "Draft the release notes for 0.4"
    Then the project's decisions don't include "Draft the release notes for 0.4"

  @projects.decisions @project-decisions
  Scenario: The sidebar folds each project to one row
    When I show the Projects page
    Then the sidebar's project "Threadbear" is folded with 4 threads
    When I open the sidebar's project "Threadbear"
    Then the sidebar lists "Rename the Settings tabs" under "Threadbear"
    When I fold the sidebar's project "Threadbear"
    Then the sidebar's project "Threadbear" is folded with 4 threads

  @projects.parts @project-parts
  Scenario: A part shows inside its parent's card and says whose it is
    When I show the Projects page
    Then the project card "Threadbear" says "(parts: Photo sync)"
    When I open the project "Photo sync"
    Then the project page says it's a part of "Threadbear"
    When I open the project "Threadbear"
    Then the project page lists the part "Photo sync"

  @projects.parts @project-parts
  Scenario: A project can be made its own again
    When I open the project "Photo sync"
    And I make the project "Photo sync" its own project
    Then the project page says it's a part of nothing

  @projects.tabs @project-decisions @gpui-pending
  Scenario: A project's page opens on its decisions, and remembers the tab picked
    When I open the project "Threadbear"
    Then the project page shows its "decisions" tab
    When I pick the project page's "project" tab
    Then the project page shows its "project" tab
    When I open the project "Walkies"
    Then the project page shows its "project" tab
    When I open the project "Threadbear"
    Then the project page shows its "project" tab

Feature: A side panel pinned to its thread
  As someone working in one project's thread while watching another's
  I want to pin a thread's panel so it stays on that thread
  So its dashboard or list doesn't go when I pick a thread without it

  Background:
    Given the app is connected to a runner with the demo threads

  # The Swift app is frozen (AGENTS.md, "The GPUI port"), so Pin is GPUI's only.
  @dock.pin @swift-pending
  Scenario: A pinned panel stays on its thread when another thread is picked
    When I open the thread "Rebuild the three landing pages"
    And I show the "deploys.previews" section of the pane
    Then the "deploys.previews" panel's menu offers "Pin"
    When I choose "Pin" from the "deploys.previews" panel's menu
    Then "Previews" is pinned to "Rebuild the three landing pages" and showing
    When I open the thread "Morning brief"
    Then "Previews" is pinned to "Rebuild the three landing pages" and showing
    And the pinned "Previews" panel lists "Home | Pricing | Docs"

  @dock.pin @swift-pending
  Scenario: A pinned panel comes back after a relaunch, and Unpin puts the ordinary one back
    When I open the thread "Rebuild the three landing pages"
    And I show the "deploys.previews" section of the pane
    And I choose "Pin" from the "deploys.previews" panel's menu
    # The GPUI app writes its settings at most every 300 ms, and the spec's
    # quit kills it without waiting for that: the pin, a moment after the
    # section showed, was lost on GitHub's slower machines.
    And I wait a moment
    And the app is quit and opened again
    Then "Previews" is pinned to "Rebuild the three landing pages" and showing
    When I choose "Unpin" from the pinned "Previews" panel's menu
    Then nothing is pinned
    When I open the thread "Rebuild the three landing pages"
    Then the pane is showing "deploys.previews"
    And the "deploys.previews" panel's menu offers "Pin"

  @dock.pin @swift-pending
  Scenario: Only a thread's own patch panels offer Pin
    When I open the thread "Rebuild the three landing pages"
    Then the "threadbear.highlights" panel's menu offers nothing

Feature: Threads nested under the thread that started them
  As someone whose threads start threads of their own
  I want those threads kept under the thread that started them
  So my Inbox and Running lists show what I started, and where the rest came from

  # Experiment nestedThreads (docs/protocol.md, "Nested threads"): the GPUI
  # app only. The Swift app is frozen and shows these threads as before.

  Background:
    Given the app is connected to a runner with the demo threads

  @sidebar.nested @nested-threads @swift-pending
  Scenario: A thread's threads sit under it, the group where its most urgent one is
    Then the "Inbox" section lists "GPUI Port Coordinator"
    And the "Running" section doesn't list "Swift app: point people to the GPUI app"
    And the group under "GPUI Port Coordinator" says "1 needs you · 5 working"
    And the group under "GPUI Port Coordinator" is open
    And the group under "GPUI Port Coordinator" lists "GPUI: the Browser pane and sign-in handoff" asking "Should the sign-in window come in front of yours?"
    And the group under "GPUI Port Coordinator" lists "Swift app: point people to the GPUI app" saying "Landing, 4th in the queue"
    And the group under "GPUI Port Coordinator" lists "HEIC: a test picture for each phone" saying "making test pictures"
    And the group under "GPUI Port Coordinator" has 2 finished

  @sidebar.nested @nested-threads @swift-pending
  Scenario: Folded, the group's needs-you dot moves to the thread that started them
    When I fold the group under "GPUI Port Coordinator"
    Then the group under "GPUI Port Coordinator" lists no threads
    And the row "GPUI Port Coordinator" has a blue dot
    When I open the group under "GPUI Port Coordinator"
    Then the group under "GPUI Port Coordinator" lists "GPUI: the Browser pane and sign-in handoff" asking "Should the sign-in window come in front of yours?"

  @sidebar.nested @nested-threads @swift-pending
  Scenario: A thread moves out from under the thread that started it
    When I choose "Move Out From Under GPUI Port Coordinator" on the row "GPUI: the Browser pane and sign-in handoff"
    Then the "Inbox" section lists "GPUI: the Browser pane and sign-in handoff"
    And the group under "GPUI Port Coordinator" says "5 working"

  @sidebar.nested @nested-threads @swift-pending
  Scenario: A thread moved out keeps its own row until it's put back
    Then the "Inbox" section lists "Fix the BrowserBear focus bug"
    When I choose "Put Back Under GPUI Port Coordinator" on the row "Fix the BrowserBear focus bug"
    Then the "Inbox" section doesn't list "Fix the BrowserBear focus bug"
    When I open the group under "GPUI Port Coordinator"
    Then the group under "GPUI Port Coordinator" lists "Fix the BrowserBear focus bug" saying "a tooling fix"

  @thread.startedby @nested-threads @swift-pending
  Scenario: A thread another thread started says which, above its transcript
    When I open the thread "GPUI: the Browser pane and sign-in handoff"
    Then the open thread says "Started by GPUI Port Coordinator · building the Browser pane"

  @sidebar.nested @nested-threads-off @swift-pending
  Scenario: With the experiment off, every thread has its own row
    Then the "Running" section lists "Swift app: point people to the GPUI app"
    And the row "GPUI Port Coordinator" heads no group

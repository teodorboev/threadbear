Feature: Thread-count milestones
  I want a small note when I reach a milestone
  So I see it once, when I am there to read it

  Background:
    Given the app is connected to a runner with the demo threads

  # The frozen Swift automation can show a tier but cannot simulate the
  # watch's count, saved tier and window activation together.
  @toast.milestone @swift-pending
  Scenario: Reaching the next thousand shows once and dismisses on a click
    When the thread count reaches 1000 with 100 already remembered
    Then the milestone says "That's 1,000 threads"
    When I dismiss the milestone
    Then no milestone is showing
    When the thread count reaches 1000 with 1000 already remembered
    Then no milestone is showing

  @toast.milestone @swift-pending
  Scenario: A milestone waits until I return
    When the thread count reaches 1000 while I am away
    Then no milestone is showing
    When I return to see the milestone
    Then the milestone says "That's 1,000 threads"

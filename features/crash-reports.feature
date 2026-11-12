Feature: Crash reports
  As the person who fixes Threadbear
  I want a crash written down with where in the code it happened
  So it can be found and fixed, while nothing I typed or opened ever goes with it

  Background:
    Given the app is connected to a runner with the demo threads

  # A scripted app keeps its panics beside its automation file and sends
  # nothing; the installed app keeps them in ~/Library/Logs/Threadbear and
  # sends them to Sentry. The Swift app's crashes go through Sentry's own SDK,
  # which is only in its release builds, so a scripted debug build of it has
  # nothing to show here.
  # Only Rust's own wording for the panics it raises goes, with its numbers:
  # any other text could hold a title, a path or a reply's words, and
  # scrubbing free text missed some (Codex's review, 12 Nov).
  @system.crash @swift-pending
  Scenario: A panic is kept with where it happened, and without the words it had
    When something in the app panics with the message:
      """
      couldn't open "Fix the share button" in /Users/mike/Secret Project/plan.md: not found
      """
    Then the app has kept that panic, written as:
      """
      panic
      """
    And the panic says where in the app's code it was

  @system.crash @swift-pending
  Scenario: Rust's own panic keeps its wording and numbers
    When something in the app panics with the message:
      """
      index out of bounds: the len is 3 but the index is 5
      """
    Then the app has kept that panic, written as:
      """
      index out of bounds: the len is 3 but the index is 5
      """

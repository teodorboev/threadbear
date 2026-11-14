@swift-pending
Feature: Tables and lists fit their words
  Background:
    Given the app is connected to a runner with the demo threads
    When I open the thread "Morning brief"

  @rich.table
  Scenario: A short table stays compact when the window gets wider
    When a reply with a short table and a list streams in
    Then the short table has a compact width
    When I widen the window around the short table
    Then the short table keeps its width

  @rich.blocks
  Scenario: List items have room between their lines
    When a reply with a short table and a list streams in
    Then the list items have six points between their lines

Feature: Buttons an agent puts in its reply
  As someone reading a brief
  I want the jobs it offers as buttons in the text
  So I can set one off as I read, without typing it

  Background:
    Given the app is connected to a runner with the demo threads

  @rich.replybuttons
  Scenario: A reply's buttons show as pills in their state, and one this Mac doesn't know as words
    When I open the thread "Morning brief"
    Then the reply's buttons read "▶︎ Fill in the form [b7k2form, start]", "✓ Look into it [c4mpanng, pressed]", "↪︎ Ask that thread [q9m4meet, send]" and "! Clear the old exports [x2gonefl, failed]"
    And the reply has an unknown button

  @rich.replybuttons
  Scenario: Pressing a button starts its thread, once
    When I open the thread "Morning brief"
    And I press the reply button "Fill in the form"
    Then the reply button "Fill in the form" reads "✓ Fill in the form [b7k2form, pressed]"

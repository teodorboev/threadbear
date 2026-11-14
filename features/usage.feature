Feature: The Usage panel and its honey pot
  As someone running threads on several logins
  I want to see what I've spent and how much room each login has left
  So I know which login to lean on and when one will run out

  Background:
    Given the app is connected to a runner with the demo threads

  @usage.spend
  Scenario: The panel says what's been spent, by the hour today or by the day this week
    When I click the honey pot in the activity bar
    Then the Usage panel is open
    And the Usage panel's tiles are "Today, Last hour, 7 days", each with a number
    And the Usage chart is "Tokens by hour"
    When I show the last 7 days on the Usage chart
    Then the Usage chart is "Tokens by day"
    And the Usage chart's key names "mike, work, mike+3, mike+4, Codex"

  @usage.pools
  Scenario: A card for each pool, with what each login has left
    When I click the honey pot in the activity bar
    Then the Usage panel has a card for each pool: "Claude, Convex, Codex, Side projects, Evenings"
    And in the "Claude" card, "mike@example.com" has "Weekly 78% left, Weekly Fable 71% left"
    And in the "Claude" card, "mike+4@example.com" says "Limit reached"
    And the "Side projects" card names each login's provider: "Codex: mike@example.com, Claude: mike+3@example.com"

  @usage.pools
  Scenario: Weekly Fable can be switched off in the panel's settings, and stays off
    When I click the honey pot in the activity bar
    Then the Usage panel's "Show Weekly Fable" switch is on
    When I turn off "Show Weekly Fable" in the Usage panel
    Then in the "Claude" card, "mike@example.com" has "Weekly 78% left"
    # The GPUI app writes its settings at most every 300 ms, and the spec's
    # quit doesn't wait for that.
    When I wait a moment
    And the app is quit and opened again
    Then the Usage panel's "Show Weekly Fable" switch is off
    And in the "Claude" card, "mike@example.com" has "Weekly 78% left"

  @rail.honeypot
  Scenario: The honey pot is filled to the room the next thread would get, and follows it
    Then the honey pot is 78% full, for "claude-main"
    And the honey pot's tooltip says "Claude: 78% left"
    And the honey pot's tooltip says "Convex: 95% left"
    When "claude-main" is down to 40% of its week
    Then the honey pot is 40% full, for "claude-main"
    And the honey pot's tooltip says "Claude: 40% left"
    When "claude-main"'s week resets
    Then the honey pot is 100% full, for "claude-main"

  # Swift's scripted form switch waits for an actual mouse-up.
  @usage.pools @swift-pending
  Scenario: Clicking the model limit switch changes the card
    When I click the honey pot in the activity bar
    And I scroll to settings in Usage
    And I press "A login's own limit on one model. Hide it if you don't use that model" in the Usage panel
    Then in the "Claude" card, "mike@example.com" has "Weekly 78% left"

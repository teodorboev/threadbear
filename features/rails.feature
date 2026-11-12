Feature: The rails down the window's edges
  As someone moving between threads, pages and panels all day
  I want one column of buttons down each side
  So I can see where I am and get anywhere in one click

  Background:
    Given the app is connected to a runner with the demo threads

  @rail.activity
  Scenario: The Threads button counts the Inbox and is lit by default
    Then the activity bar's "Threads" button is lit
    And the activity bar's "Threads" button shows how many threads are in the Inbox

  @rail.activity
  Scenario: Picking a view in the activity bar shows it
    When I click "Projects" in the activity bar
    Then the sidebar shows the "projects" view
    And the activity bar's "Projects" button is lit
    When I click "Threads" in the activity bar
    Then the sidebar shows the "inbox" view

  @rail.activity
  Scenario: The foot of the bar opens Connectors, Runners and Updates under the threads
    Then the activity bar has a "Connectors" button
    And the activity bar has a "Runners" button
    And the activity bar has a "Updates" button
    When I click "Runners" at the foot of the activity bar
    Then the Runners panel is open
    And the activity bar's "Runners" button is lit
    When I click "Updates" at the foot of the activity bar
    Then the Updates panel is open
    And the Runners panel is closed
    When I click "Updates" at the foot of the activity bar
    Then the Updates panel is closed

  @rail.dots
  Scenario: Connectors has the accent dot while a service is signed out, and a grey one while the gateway can't be reached
    Then the activity bar's "Connectors" button has the accent dot
    When the gateway can't be reached
    Then the activity bar's "Connectors" button has the grey dot
    When every service on the gateway is signed in
    Then the activity bar's "Connectors" button has no dot

  @rail.dots
  Scenario: Updates has a grey dot while a CLI has a new version, and the accent one when only I can act
    Then the activity bar's "Updates" button has the grey dot
    When the runner says its launcher has changed
    Then the activity bar's "Updates" button has the accent dot

  @rail.right
  Scenario: The right rail has the thread's Highlights and Files
    When I open the thread "Morning brief"
    Then the right rail has a "Highlights" button
    And the right rail has a "Files" button

  @rail.right
  Scenario: A rail button shows its panel, and hides it again
    When I open the thread "Morning brief"
    Then the panels are laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"
    When I click "Highlights" in the right rail
    Then the panels aren't laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"
    When I click "Highlights" in the right rail
    Then the "Highlights" button in the right rail is lit

  @rail.dots
  Scenario: Highlights carries a dot while the agent's asks are open
    When I open the thread "Ship the beta to TestFlight"
    Then the "Highlights" button in the right rail has a dot
    When I open the thread "Morning brief"
    Then the "Highlights" button in the right rail has no dot

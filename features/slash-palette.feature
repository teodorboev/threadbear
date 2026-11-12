Feature: The slash command palette
  As someone who knows there's a command for this
  I want to find it by typing a slash
  So I can run it, or fill in its arguments, without leaving the box

  Background:
    Given the app is connected to a runner with the demo threads
    And I open the thread "Morning brief"

  @composer.slash
  Scenario: A slash opens the palette with every command, in categories
    When I type "/" in the composer
    Then the palette is open on "All"
    And the palette's categories say "All 15 | Threadbear 2 | Claude Code 6 | Skills 7"
    And the palette lists "/model" first
    And the palette has the command "/code-review"

  @composer.slash
  Scenario: The picked command is described in full
    When I type "/code" in the composer
    Then the palette's pick is "/code-review"
    And the palette's details say the usage "/code-review [low|medium|high] [target]"
    And the palette's details say they're from "Your skill"

  @composer.slash
  Scenario: Typing narrows the list, names first
    When I type "/co" in the composer
    Then the palette lists "/compact" first
    And the palette has the command "/computercub"

  @composer.slash
  Scenario: A category narrows the list
    When I type "/" in the composer
    And I click the palette's "Skills" category
    Then the palette is open on "Skills"
    And the palette's commands are "/code-review | /computercub | /cloudflare | /claude-api | /deslop | /delegate | /ship"

  @composer.slash
  Scenario: Recent shows once a command has been picked
    When I type "/mod" in the composer
    Then the palette's pick is "/model"
    When I press Tab
    Then the composer says "/model "
    And the palette is closed
    When I type "/" in the composer, replacing what's there
    Then the palette's categories say "All 15 | Threadbear 2 | Claude Code 6 | Skills 7 | Recent 1"

  # Skills named partway through a message (9 Nov 2026, docs/guide.md):
  # only the ones the agent can run there, and a pick goes into the words.
  @composer.slash
  Scenario: A slash partway through a message lists the skills the agent can run
    When I type "then tidy it with /de" in the composer
    Then the palette is open on "All"
    And the palette's commands are "/deslop | /delegate | /code-review | /claude-api"

  @composer.slash
  Scenario: Return partway through a message puts the skill in the words without sending
    When I type "then tidy it with /desl" in the composer
    Then the palette's pick is "/deslop"
    When I press Return
    Then the composer says "then tidy it with /deslop "
    And the palette is closed
    And the transcript doesn't show my message "then tidy it with"

  @composer.slash
  Scenario: A slash in other words with nothing to match leaves the palette shut
    When I type "look in /tmp" in the composer
    Then the palette is closed

  # folders.commands: the list for the folder and login it would start on.
  @composer.slash
  Scenario: The new-thread page's box lists the commands before the thread exists
    When I start a new thread
    And I type "/" in the composer
    Then the palette is open on "All"
    And the palette's categories say "All 15 | Threadbear 2 | Claude Code 6 | Skills 7"
    And the palette has the command "/code-review"

  @composer.slash.keys
  Scenario: The arrows move the pick and stop at the ends
    When I type "/" in the composer
    Then the palette's pick is "/model"
    When I press the down arrow
    And I press the down arrow
    Then the palette's pick is "/clear"
    When I press the up arrow
    Then the palette's pick is "/effort"
    When I press the up arrow
    And I press the up arrow
    Then the palette's pick is "/model"

  @composer.slash.keys
  Scenario: Return runs the picked command
    When I type "/cle" in the composer
    Then the palette's pick is "/clear"
    When I press Return
    Then the palette is closed
    And the transcript shows my message "/clear"
    And the composer is empty

  @composer.slash.keys
  Scenario: Tab fills it in, for its arguments
    When I type "/cod" in the composer
    Then the palette's pick is "/code-review"
    When I press Tab
    Then the composer says "/code-review "
    And the palette is closed

  @composer.slash.keys
  Scenario: Escape closes it until the words change
    When I type "/mod" in the composer
    Then the palette is open on "All"
    When I press Escape
    Then the palette is closed
    And the composer says "/mod"
    When I type "e" in the composer
    Then the palette is open on "All"

  # The Swift palette is a panel window of its own whose edges follow the real
  # pointer (NSEvent.mouseLocation), which a script can't move without taking
  # the Mac's own, so these are driven on the GPUI app, with real presses and
  # moves on its edges.
  @composer.slash.resize @swift-pending
  Scenario: Its top edge drags it taller, and it stays that tall next time
    When I type "/" in the composer
    And I note the palette's size
    And I drag the palette's top edge 60 points up
    Then the palette is 60 points taller than it was
    When I press Escape
    And I type "m" in the composer
    Then the palette is still 60 points taller than it was

  @composer.slash.resize @swift-pending
  Scenario: Default Size, from its right-click menu, puts it back
    When I type "/" in the composer
    And I note the palette's size
    And I drag the palette's top edge 60 points up
    And I choose "Default Size" from the palette's right-click menu
    Then the palette is the size it was

  # The detail keeps 200 points, so in a palette as narrow as the box the
  # list has nowhere to grow: the palette is widened first.
  @composer.slash.resize @swift-pending
  Scenario: Its sides drag it wider, and the line after the list drags the list wider
    When I type "/" in the composer
    And I note the palette's size
    And I drag the palette's right edge 150 points right
    Then the palette is 300 points wider than it was
    When I note the palette's size
    And I drag the list's edge 40 points right
    Then the list is 40 points wider than it was

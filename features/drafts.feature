Feature: Drafts in a reply
  As someone an agent writes messages for
  I want a Slack message or an email it drafted shown the way it'll go out
  So I can read it through and copy it straight into Slack or Mail

  Background:
    Given the app is connected to a runner with the demo threads

  @rich.draft
  Scenario: A Slack draft keeps every line where the agent put it, in Slack's formatting
    When I open the thread "Morning brief"
    And a reply with a Slack draft streams in
    Then the reply shows a "Slack draft" box with the lines "Threadbear, 6 Nov", "• one link" and "• two"
    And in the draft "Threadbear, 6 Nov" is bold and "link" links to "https://x.dev"

  @rich.draft
  Scenario: An email draft keeps its blank lines and links its addresses
    When I open the thread "Morning brief"
    And a reply with an email draft streams in
    Then the reply shows an "Email draft" box with the lines "Hi Sam,", "", "See https://x.dev/a for *this*.", "" and "Mike"
    And in the draft "https://x.dev/a" links to "https://x.dev/a"

  @rich.draft
  Scenario: A draft whose fence hasn't closed yet stays code
    When I open the thread "Morning brief"
    And a reply with an unfinished Slack draft streams in
    Then the reply shows no draft box

  # The GPUI app only puts HTML beside the words on the Mac's clipboard so
  # far: Windows' and Linux's need code of their own (draft_boxes.rs).
  @rich.draft.copy @windows-pending @linux-pending
  Scenario: Copy puts the draft on the clipboard with its bold and links, and says so
    When I open the thread "Morning brief"
    And a reply with a Slack draft streams in
    And I press the draft's Copy
    Then the draft's Copy reads "Copied"
    And the clipboard holds the lines "*Threadbear, 6 Nov*", "• one link (https://x.dev)" and "• two"
    And the clipboard's HTML has '<b>Threadbear, 6 Nov</b><br>• one <a href="https://x.dev">link</a><br>• two'

  @rich.draft.select
  Scenario: Words in a draft can be selected, and Copy on the bar copies them
    When I open the thread "Morning brief"
    And a reply with a Slack draft streams in
    And I select "one link" in the draft
    Then the bar shows over the words
    When I press "copy" on the bar over "one link" in the draft
    Then the clipboard holds just "one link"

  # A scripted Swift app's ⌘C goes to the Mac's own clipboard, not the
  # instance's own that `clipboard` reads (selected-words.feature), so it's
  # only read back here from the GPUI app.
  @rich.draft.select @swift-pending
  Scenario: ⌘C copies words selected in a draft
    When I open the thread "Morning brief"
    And a reply with a Slack draft streams in
    And I select "one link" in the draft
    And I press ⌘C
    Then the clipboard holds just "one link"

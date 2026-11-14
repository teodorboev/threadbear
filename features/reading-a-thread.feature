Feature: Reading a thread
  As someone picking up where an agent got to
  I want the conversation laid out with the working folded away
  So I can see what was said and what came of it

  Background:
    Given the app is connected to a runner with the demo threads

  @transcript.user @transcript.reply
  Scenario: The conversation reads top to bottom
    When I open the thread "Make uploads resume after a crash"
    Then the transcript shows my message "Uploads start again from zero if the app quits"
    And the transcript shows a reply starting "Uploads now pick up where they stopped"
    And my message "Uploads start again from zero" comes before the reply "Uploads now pick up"

  @transcript.workedfor
  Scenario: What the agent did is folded into one line
    When I open the thread "Make uploads resume after a crash"
    Then the agent's work is folded into a line saying "Worked for 2m 14s"

  @transcript.workedfor @transcript.tool
  Scenario: Opening that line shows each thing it did
    When I open the thread "Make uploads resume after a crash"
    And I open the line saying "Worked for 2m 14s"
    Then the transcript shows a step starting "Read"

  @transcript.turnfooter
  Scenario: A finished turn says how long it took and what it cost
    When I open the thread "Make uploads resume after a crash"
    Then a turn ends with a line saying "2m 14s, $0.38"

  @transcript.changes
  Scenario: A turn that edited files says so
    When I open the thread "Make uploads resume after a crash"
    Then the transcript has a card saying "Edited 2 files"

  @transcript.error
  Scenario: A failed turn says what went wrong
    When I open the thread "Make a poster for the meetup"
    Then the transcript shows an error

  @transcript.thinking
  Scenario: The agent's thinking opens to what it thought
    When I open the thread "Why is the build slow on CI?"
    And I open the line saying "Read 2 files, ran a command"
    And I open the thinking that starts "The slower run"
    Then the thinking that starts "The slower run" is open

  @transcript.tool @transcript.tooloutput
  Scenario: A tool opens to its output and closes again
    When I open the thread "Why is the build slow on CI?"
    And I open the step called "Download the last two CI logs"
    Then the step called "Download the last two CI logs" is open and taller than one line
    When I close the step called "Download the last two CI logs"
    Then the step called "Download the last two CI logs" is one line again

  @transcript.diff
  Scenario: An edit opens to its diff, and to every line of it
    When I open the thread "Make a poster for the meetup"
    And I open the line saying "Worked for 48s"
    And I open the step called "Edit 3 files"
    Then the step called "Edit 3 files" is open and taller than one line
    When I show every line of the step called "Edit 3 files"
    Then the step called "Edit 3 files" got taller

  @transcript.changes
  Scenario: A file in the changes card opens to its diff
    When I open the thread "Make a poster for the meetup"
    And I open the first file in the card saying "Edited 3 files"
    Then the card saying "Edited 3 files" got taller

  @transcript.images
  Scenario: Pictures the agent made show in the transcript
    When I open the thread "Make a poster for the meetup"
    Then the transcript shows 2 rows of pictures

  @transcript.working @transcript.reply
  Scenario: A running turn shows the working line and its reply as it streams
    When I open the thread "Add dark mode to the dashboard"
    And a reply starts streaming in
    Then the working line shows
    And the streaming reply grows

  # A real thread's wait lines (a92fcf9a) wrapped onto several lines in the
  # GPUI app and were drawn over the replies either side.
  @transcript.gap
  Scenario: Wait lines sit between the replies, not over them
    Given a thread the agent picked up twice as background jobs finished, called "Land the folder it runs in"
    When I open the thread "Land the folder it runs in"
    Then the transcript shows a wait line starting "44s later · /Users/mike/.delegate/runs/"
    And the transcript shows a wait line starting "3 min later · cd /Users/mike/dev/me/threadbear-threads/slot-5"
    And each wait line's words are one line inside its own row

  @transcript.rowmenu
  Scenario: A reply's words can be copied from its menu
    When I open the thread "Make uploads resume after a crash"
    And I choose "Copy message" from the menu of the reply starting "Uploads now pick up"
    Then the clipboard holds "Uploads now pick up where they stopped"

  # A block's play icon putting its command at the prompt is in terminal.feature.
  @manual @gpui-pending @terminal.putcommand
  Scenario: Only the thread in the main panel puts its commands in the Terminal
    When I open a thread whose reply has a bash block with "npm test"
    And another Mac's thread with a bash block is open in a panel of its own
    Then that panel's block has its copy icon and no play icon
    And a block in another of this Mac's threads open in a panel of its own has no play icon either

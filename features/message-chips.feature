Feature: Chips in my messages
  As someone who points at pictures, cards and other threads as I write
  I want each one in my message as a chip, not as its raw words
  So I can see what I pointed at, and get back to it with a click

  Background:
    Given the app is connected to a runner with the demo threads

  @rich.picturechips @transcript.user
  Scenario: A picture among my words is a chip in my message, with the picture on a hover
    When I open the thread "Morning brief"
    And I type "compare " in the composer
    And I paste the picture into the composer
    And I type "with the old one" in the composer
    Then the composer holds 1 picture
    When I press Return
    Then my message "compare" shows the chips "picture: Picture 1"
    And resting on the chip "Picture 1" in my message shows its card
    And pressing the chip "Picture 1" in my message opens picture 1 of 1

  @rich.cardchips @rich.threadchips @transcript.user
  Scenario: A card and another thread linked in my words are chips with their titles
    When I open the thread "Morning brief"
    And I send "Look at [Fix the share button](https://www.notion.so/team/fix-share-1) and [Tidy the imports](thread:demo_tidy_imports)"
    Then my message "Look at" shows the chips "card: Fix the share button" and "thread: Tidy the imports"
    When I follow "thread:demo_tidy_imports" in my message "Look at"
    Then the open thread is "Tidy the imports in the uploader"

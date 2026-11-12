Feature: An archived or hibernated thread's page flies to where it went
  As someone working through the inbox
  I want to see the thread I put away go into its heading
  So I know where it went while the next one opens

  Background:
    Given the app is connected to a runner with the demo threads
    # Folded, the Inbox leaves room for the headings below it to be in sight.
    And the "inbox" section is folded away

  @juice.archiveflight
  Scenario: Archiving the open thread drops its page into the Archived heading
    Given I open the thread "Morning brief"
    When I archive the open thread
    Then the page of "Morning brief" flies from the transcript into the "archived" heading
    And another thread opens in its place

  @juice.hibernateflight
  Scenario: Hibernating the open thread sinks its page into Hibernating's moon
    Given I open the thread "Morning brief"
    When I hibernate the open thread "Morning brief" for an hour
    Then the page of "Morning brief" flies from the transcript into the "hibernate" heading
    And another thread opens in its place

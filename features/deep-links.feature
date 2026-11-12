Feature: Links into the app from outside
  As someone whose agents work in my browser too
  I want BrowserBear's Open thread pill on an agent's tab to bring its thread up here
  So I go straight from the tab to the thread that's driving it

  Background:
    Given the app is connected to a runner with the demo threads

  @nav.deeplink
  Scenario: A threadbear link opens its thread
    When a threadbear link to "Morning brief" comes from outside the app
    Then the open thread is "Morning brief"

  @nav.deeplink
  Scenario: A threadbear link shows its thread in the Inbox view, whatever view was showing
    Given I click "Projects" in the activity bar
    When a threadbear link to "Rename the Settings tabs" comes from outside the app
    Then the sidebar shows the "inbox" view
    And the open thread is "Rename the Settings tabs"

  @nav.deeplink @remote-threads
  Scenario: A threadbear link opens another Mac's thread too
    When a threadbear link to "Check the photo library backup" comes from outside the app
    Then the open thread is "Check the photo library backup"

  # A link is input from outside the app: anything but a thread link under
  # the scheme (the phone's pairing link, say) does nothing at all.
  @nav.deeplink
  Scenario: A threadbear link that isn't to a thread does nothing
    When I open the thread "Morning brief"
    And the link "threadbear://pair?url=https%3A%2F%2Fx.convex.cloud&token=tb_1" comes from outside the app
    Then the open thread is "Morning brief"

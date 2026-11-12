Feature: The Connectors panel
  As someone whose agents use services through my Connectors gateway
  I want to see which services are signed in, and sign one back in from there
  So an agent never stalls on a service I didn't know had signed out

  Background:
    Given the app is connected to a runner with the demo threads

  @connectors.panel
  Scenario: Each service says whether it's signed in, and a signed-out one has Connect
    When I open the Connectors panel
    Then the Connectors panel's heading is "bruce"
    And the Connectors panel lists "Notion: Signed in"
    And the Connectors panel lists "Sentry: Signed out [Connect]"
    And the Connectors panel lists "  The sign-in expired."

  @connectors.panel
  Scenario: It says when the gateway can't be reached, and catches up once it's back
    When I open the Connectors panel
    And the gateway can't be reached
    Then the Connectors panel lists "Can't reach bruce: timed out."
    When every service on the gateway is signed in
    Then the Connectors panel lists "Sentry: Signed in"
    And the Connectors panel doesn't list "Can't reach bruce: timed out."

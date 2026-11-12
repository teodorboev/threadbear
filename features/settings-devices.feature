Feature: Settings › Phone & browser
  As someone who reads and answers threads away from the Mac
  I want to pair my phone and browsers from Settings, and see what's paired
  So I can sign one out that I don't use any more

  Background:
    Given the app is connected to a runner with the demo threads

  @settings.devices
  Scenario: The paired phones and browsers, with when each runs out
    When I open Settings on "devices"
    Then Settings offers to add a phone or browser
    And Settings says when "iPhone" was paired and until when
    And Settings says "Safari" has run out

  @settings.devices
  Scenario: Revoking a browser signs it out
    When I open Settings on "devices"
    And Settings lists "Chrome" as paired
    And I revoke "Chrome" in Settings
    Then Settings doesn't list "Chrome" as paired

  @settings.devices
  Scenario: A revoke the backend doesn't take puts the browser back, saying why
    Given the backend won't answer the next pairing change
    When I open Settings on "devices"
    And Settings lists "Chrome" as paired
    And I revoke "Chrome" in Settings
    Then Phone & browser says "The backend didn't answer. Try again in a moment."
    And Settings lists "Chrome" as paired

  @settings.devices
  Scenario: Show the code pairs a phone
    When I open Settings on "devices"
    And Settings lists "iPhone" as paired
    And I ask Settings for the code to pair a phone
    Then Settings shows the pairing code
    And Settings lists 2 paired iPhones
    When I close the pairing code
    Then Settings doesn't show the pairing code

  @settings.devices
  Scenario: With nothing mirrored to Convex, it says why nothing can pair
    Given the runner isn't mirroring threads because "sync is off in runner.json"
    When I open Settings on "devices"
    Then Phone & browser says "Your threads aren't mirrored to Convex yet (sync is off in runner.json), so there's nothing for a phone or browser to read."

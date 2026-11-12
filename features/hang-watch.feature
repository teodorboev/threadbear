Feature: The hang watch
  As someone whose app sometimes freezes
  I want a freeze caught while it's happening, with what the app was stuck doing
  So the cause can be found and fixed, not guessed at from where the logs stop

  Background:
    Given the app is connected to a runner with the demo threads

  # A scripted app watches into a folder beside its automation file and sends
  # nothing; the installed app does the same into ~/Library/Logs/Threadbear/hangs
  # and sends the long ones to Sentry at its next launch.
  @system.hangwatch
  Scenario: A stall of four seconds or more is noted with a sample of the stuck main thread
    When the app's main thread is stuck for 10 seconds
    Then the hang watch has noted a stall of about 10 seconds
    And its sample shows the main thread stuck in the app's scripted control

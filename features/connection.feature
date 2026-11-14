Feature: Staying connected to the runner
  As someone whose runner restarts when it updates
  I want the app to reconnect by itself
  So I never have to do anything about it

  Background:
    Given the app is connected to a runner with the demo threads

  @foundation.rpc
  Scenario: The app reconnects when the runner restarts
    When I open the thread "Morning brief"
    And the runner restarts
    Then the app is connected again
    And the transcript still shows the thread "Morning brief"

  @foundation.rpc
  Scenario: A malformed summary cannot hide the readable threads after reconnecting
    When the runner's next list includes malformed fields and rows
    And the runner restarts
    Then the app is connected again
    And the sidebar shows the recovered thread title

  @banner.connection
  Scenario: The bar says when the runner can't be reached, and goes once it's back
    When the runner goes away without a word for 8 seconds
    Then the bar says "Can't reach the runner."
    And the app is connected again
    And there's no banner once the runner is back

  @thread.loadfailure
  Scenario: A thread that can't be read says so, and Try again reads it
    Given the thread "Morning brief" can't be read once
    When I open the thread "Morning brief", which fails to load
    Then the transcript says it couldn't be loaded
    When I press Try again on the transcript
    Then the transcript shows the thread "Morning brief"

  # The runner restarts onto main several times a day. A message sent while it's
  # away waits in the transcript and goes once it's back, rather than bouncing
  # back into the box (Mike, 9 Nov).
  # Gone long enough for the typing: a scripted app on a check host's software
  # renderer types about ten characters a second.
  @transcript.pending
  Scenario: A message sent while the runner restarts waits for it and goes once it's back
    When I open the thread "Morning brief"
    And the runner restarts and is gone for 12 seconds
    And the app has lost the runner
    And I send "Check the backups once you're back"
    Then the transcript's last row reads "(waiting for the runner) Check the backups once you're back"
    And the composer is empty
    And the app is connected again
    And the transcript shows my message "Check the backups once you're back" just once
    And the reply streams in and the thread goes back to idle

  @transcript.pending
  Scenario: With no restart under way, a message the runner can't take stays in the box
    When I open the thread "Morning brief"
    And the runner goes away without a word for 8 seconds
    And the app has lost the runner
    And I send "Are the backups done?"
    Then the error bar says "Not connected to the runner, so that wasn't sent. It's still in the box."
    And the composer says "Are the backups done?"

  @banner.error
  Scenario: A message cut off on its way names its thread, and the bar stays once I've moved on
    Given the runner takes 3 seconds to answer each message
    When I open the thread "Morning brief"
    And I send "Are the backups done?"
    And the runner goes away without a word for 8 seconds
    Then the error bar says "Lost the connection to the runner. Your message to “Morning brief” wasn't sent. It's back in its box."
    And the composer says "Are the backups done?"
    When I open the thread "Rename the Settings tabs"
    Then the error bar says "Lost the connection to the runner. Your message to “Morning brief” wasn't sent. It's back in its box."
    When I dismiss the error bar
    Then there's no error bar

  @banner.connection
  Scenario: While a restart waits for threads, the bar says how many and which
    When the runner says it will restart once "Rename the Settings tabs" finishes
    Then the bar says quietly "The runner will restart when 1 thread finishes."
    And the bar's tip says "Waiting for Rename the Settings tabs"

  @banner.error
  Scenario: A warning from the runner shows in the error bar
    When the runner warns "The runner couldn't update to main at abc1234: this machine is short of disk space. It tries again by itself once that clears."
    Then the error bar says "The runner couldn't update to main at abc1234: this machine is short of disk space. It tries again by itself once that clears."

  @banner.connection
  Scenario: A CLI update that failed says so in the bar until it's dismissed
    When the runner's update of Claude Code fails with "the installer exited with 1"
    Then the bar says quietly "Couldn't update Claude Code to 2.1.312: the installer exited with 1"
    When I dismiss the bar
    Then there's no banner once the runner is back

  @banner.connection
  Scenario: Copying the transcript from a menu says it's done in the bar
    When I open the thread "Morning brief"
    And I choose "Copy Transcript as Markdown" from the menu bar
    Then the bar says quietly "Transcript copied"

  @system.applog
  Scenario: The app's log says when the connection went and came back
    When the runner restarts
    Then the app is connected again
    And the app's log says "lost the connection to the runner"
    And the app's log says "connected to runner"

  # Mike, 12 Nov: mid-restart the sidebar showed "Can't reach the runner:
  # Couldn't reach the runner at ws://127.0.0.1:8797/rpc: IO error: Connection
  # refused (os error 61). Trying again in 8s." under the banner's quiet line.
  @banner.connection
  Scenario: While the runner restarts, the only word is that it's restarting
    When I open the thread "Morning brief"
    And the runner restarts and is gone for 5 seconds
    And the app has lost the runner
    Then until it's back the window says only that the runner is restarting
    And there's no banner once the runner is back

  # The Swift app warns at once; Mike asked for a few seconds' grace (12 Nov),
  # as this Mac's runner is usually back by then.
  @banner.connection @swift-pending
  Scenario: A runner that goes without a word is mentioned only after a few seconds, in plain words
    When the runner goes away without a word for 12 seconds
    And the app has lost the runner
    Then for 2 seconds there's no banner
    And the bar says "Can't reach the runner."
    And nothing in the window shows the connection's raw error

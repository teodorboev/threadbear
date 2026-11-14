Feature: Settings › Keyboard
  As someone who works from the keyboard
  I want to change any command's key by pressing the new one
  So the keys are where my hands expect them, in the app and its menus

  Background:
    Given the app is connected to a runner with the demo threads

  @settings.keyboard
  Scenario: Every command is listed with its key
    When I open Settings on "keyboard"
    Then Settings lists "New Thread: ⌘N" under Keyboard
    And Settings lists "Next Inbox Thread: ⌥⌘↓" under Keyboard
    And Settings lists "Hibernate: none" under Keyboard

  @settings.keyboard
  Scenario: Pressing a free key gives it to the command, and to its menu item
    When I open the thread "Morning brief"
    And I open Settings on "keyboard"
    And I click the key for "archive" in Settings
    Then Settings is listening for a new key for "Archive"
    When I press the keys "cmd+y" in Settings
    Then Settings lists "Archive: ⌘Y (changed)" under Keyboard
    And Settings isn't listening for a key
    And the "Thread" menu has "Archive" on "⌘Y"

  @settings.keyboard
  Scenario: Another command's key is asked about, and can be taken from it
    When I open Settings on "keyboard"
    And I click the key for "archive" in Settings
    And I press the keys "cmd+n" in Settings
    Then Settings asks "⌘N is already New Thread."
    When I use the key for this one instead in Settings
    Then Settings lists "Archive: ⌘N (changed)" under Keyboard
    And Settings lists "New Thread: none (changed)" under Keyboard

  @settings.keyboard
  Scenario: A key that would type, or one that's kept for something else, can't be had
    When I open Settings on "keyboard"
    And I click the key for "archive" in Settings
    And I press the keys "opt+e" in Settings
    Then Settings asks "Include ⌘ or ⌃, otherwise that key would type instead."
    When I press the keys "cmd+c" in Settings
    Then Settings asks "⌘C is taken by Copy, which can't be changed."
    When I press the keys "escape" in Settings
    Then Settings isn't listening for a key
    And Settings lists "Archive: ⌘E" under Keyboard

  @settings.keyboard
  Scenario: Delete takes a command's key away, and Reset puts it back
    When I open Settings on "keyboard"
    And I click the key for "stop" in Settings
    And I press the keys "delete" in Settings
    Then Settings lists "Stop: none (changed)" under Keyboard
    When I reset the key for "stop" in Settings
    Then Settings lists "Stop: ⌘." under Keyboard

  @settings.keyboard
  Scenario: Reset All puts every key back as Threadbear comes
    When I open Settings on "keyboard"
    And I click the key for "archive" in Settings
    And I press the keys "cmd+y" in Settings
    Then Settings lists "Archive: ⌘Y (changed)" under Keyboard
    When I reset all the keys in Settings
    Then Settings lists "Archive: ⌘E" under Keyboard
    And Settings has nothing to reset

  @settings.keyboard
  Scenario: A key pressed for a command doesn't also do what it does now
    When I open the thread "Morning brief"
    And I open Settings on "keyboard"
    And I click the key for "stop" in Settings
    And I press the keys "cmd+e" in Settings
    Then Settings asks "⌘E is already Archive."
    And the open thread is "Morning brief"

  @settings.keyboard
  Scenario: A change to shortcuts.json shows on the page, and one that won't read says so
    When I open Settings on "keyboard"
    And shortcuts.json gives "archive" the key "cmd+u"
    Then Settings lists "Archive: ⌘U (changed)" under Keyboard
    When shortcuts.json holds "{ an agent halfway through"
    Then Keyboard says "shortcuts.json isn't valid JSON, so these are the last keys that worked."
    And Settings lists "Archive: ⌘U (changed)" under Keyboard

  # The Swift page keeps its search in the view's own state, out of the
  # script's reach.
  @settings.keyboard @swift-pending
  Scenario: The search finds commands by name, menu or key
    When I open Settings on "keyboard"
    And I search the keys for "zoom"
    Then Settings lists "Zoom In: ⌘=" under Keyboard
    And Settings doesn't list "Archive: ⌘E" under Keyboard

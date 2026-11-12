Feature: The keys, shown while ⌘ is held
  As someone learning the app's keys
  I want each button's key to show beside it while I hold ⌘
  So I can learn them at a glance without them printed on the buttons all the time

  Background:
    Given the app is connected to a runner with the demo threads

  @nav.hints
  Scenario: Holding ⌘ shows each button's key beside it, and letting go puts them away
    Then no keys show beside the buttons
    When I hold ⌘
    Then the keys show beside the buttons: "Threads ⌘1 | Scheduled ⌘2 | Projects ⌘3 | Librarian ⌘4 | Settings ⌘, | New thread ⌘N"
    When I let go of ⌘
    Then no keys show beside the buttons

  @nav.hints
  Scenario: A key changed in shortcuts.json shows as it's changed
    When shortcuts.json gives "newThread" the key "cmd+t"
    And I hold ⌘
    Then the keys show beside the buttons: "New thread ⌘T"

  # The Swift app's scripted keys go straight to the menu bar
  # (`performKeyEquivalent`), past the event watch that puts the keys away,
  # so only the GPUI app can be pressed through here.
  @nav.hints @swift-pending
  Scenario: Using ⌘ for a shortcut puts the keys away
    When I hold ⌘
    And I press "1" with "cmd"
    Then no keys show beside the buttons

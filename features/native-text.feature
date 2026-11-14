Feature: The Mac's text services use the current selection
  As someone editing a message
  I want the system's text services to use the words I selected
  So a late answer cannot replace another draft

  # The frozen Swift driver has no native requestor/selection snapshot commands.
  # AppKit requestors and system popovers are Mac-only; the Terminal focus
  # scenario also runs on Windows/Linux through the shared Rust selection getter.
  Background:
    Given the app is connected to a runner with the demo threads
    When I open the thread "Morning brief"

  @rich.textservices @swift-pending @windows-pending @linux-pending @real-screen
  Scenario: A native Service receives Unicode and returns one undoable edit
    When I select "café 🐾" for a Mac Service
    Then the native Service receives "café 🐾"
    When the native Service returns "coffee 🐾"
    Then the composer says "coffee 🐾"
    And the native Service cannot return another answer
    When I undo the Service edit
    Then the composer says "café 🐾"

  @rich.textservices @swift-pending @windows-pending @linux-pending @real-screen
  Scenario: A late answer cannot overwrite a later draft
    When I select "original words" for a Mac Service
    Then the native Service receives "original words"
    When I select "later draft" for a Mac Service
    And the native Service returns "wrong answer"
    Then the composer says "later draft"

  @rich.textservices @swift-pending @windows-pending @linux-pending @real-screen
  Scenario: A send-only Service cannot replace words
    When I select "read these words" for a Mac Service
    Then a send-only native Service receives "read these words"
    And the native Service cannot return another answer
    And the composer says "read these words"

  @rich.textservices @swift-pending
  Scenario: The Terminal's keyboard focus suppresses an older reply selection
    Given the thread "Morning brief" works in a folder of its own
    When a reply saying "older reply words" streams in
    And I select "older reply words" in the reply
    Then the shared text selection is "older reply words"
    When I give the Terminal the keyboard
    Then no reply words are offered to system text services
    And the Terminal keeps the Mac Look Up shortcut

  @rich.textservices @swift-pending @windows-pending @linux-pending @real-screen
  Scenario: A reply replacement clears the system Services snapshot without a focus change
    When a reply saying "old words to be replaced" streams in
    And I mouse-select "old words to be replaced" in the reply
    Then the automatic Services snapshot contains "old words to be replaced"
    When that reply is replaced with "a different reply"
    Then the automatic Services snapshot has no old reply words

  @rich.textservices @swift-pending @windows-pending @linux-pending @real-screen
  Scenario: The Look Up shortcut reaches the focused message box
    When I select "dictionary" for a Mac Service
    And I use the Mac Look Up shortcut
    Then the selected words are offered to Look Up once

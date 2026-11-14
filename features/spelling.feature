Feature: Spelling in the message box
  As someone who types and dictates into the message box all day
  I want misspelled words marked and fixed the way any Mac text box does it
  So my messages say what I meant

  # The Mac's own spell checker, which the app doesn't have on Windows or
  # Linux. The Swift app's NSTextView did all of this, but its script has no
  # command to read the marks or the right-click menu, and that app is frozen.

  Background:
    Given the app is connected to a runner with the demo threads
    And I open the thread "Morning brief"

  @composer.spelling @swift-pending @windows-pending @linux-pending
  Scenario: A misspelled word is marked once it's typed, not while it is
    When I type "I saw teh catt" in the composer
    Then the composer marks only "teh" as misspelled

  @composer.spelling @swift-pending @windows-pending @linux-pending
  Scenario: The right-click menu offers the spell checker's guesses
    When I type "I saw teh cat" in the composer
    Then the composer's right-click menu on "teh" offers "the"
    When I choose "the" from the composer's right-click menu on "teh"
    Then the composer says "I saw the cat"

  @composer.spelling @swift-pending @windows-pending @linux-pending
  Scenario: A misspelled word is corrected as the next word starts
    Given "Spelling and Grammar › Correct Spelling Automatically" is ticked in the composer's right-click menu
    When I type "I saw teh" in the composer
    And I type " " in the composer
    Then the composer says "I saw the "

  @composer.spelling @swift-pending @windows-pending @linux-pending
  Scenario: Transformations change the word the cursor is in
    When I type "make it loud" in the composer
    And I choose "Transformations › Make Upper Case" from the composer's right-click menu on "loud"
    Then the composer says "make it LOUD"

  @composer.spelling @swift-pending @windows-pending @linux-pending
  Scenario: The right-click menu has NSTextView's submenus
    When I type "hello" in the composer
    Then the composer's right-click menu on "hello" has "Cut | Copy | Paste | Spelling and Grammar › Check Document Now | Spelling and Grammar › Check Spelling While Typing | Substitutions › Smart Quotes | Substitutions › Text Replacement | Transformations › Capitalize | Speech › Start Speaking"

Feature: The small motions
  As someone clicking about the app all day
  I want what I do to answer with a small motion where it lands
  So it feels alive without ever slowing me down

  # Mike picked these from the Swift app's, filmed beside GPUI's, on 13 Nov
  # (the bears he passed over). Most are only seen on video
  # (features/tools/motion-clip.ts); the GPUI app's dump says where the two
  # that hang on state went (`juice`). The Swift app's dump doesn't.

  Background:
    Given the app is connected to a runner with the demo threads

  @sidebar.rowmotion @swift-pending
  Scenario: A row archived from its button slides off before it goes
    Given I open the thread "Morning brief"
    When I hover the row "Tidy the imports in the uploader"
    And I press the row button "Archive"
    Then the row "Tidy the imports in the uploader" slid off

  @card.done @swift-pending
  Scenario: A card that arrives while I'm watching settles in
    Given I open the thread "Morning brief"
    When I send "Any follow-ups?"
    Then the card that came settled in

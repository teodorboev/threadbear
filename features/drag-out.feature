Feature: Dragging pictures and files out
  As someone sharing what an agent made
  I want to drag a picture or a file straight out of the app
  So I can post it or keep it without opening it first

  Background:
    Given the app is connected to a runner with the demo threads

  # In the window it's the app's own drag, which the thread takes as a dropped
  # file; out of the window it's handed to the system as the file itself,
  # tried with a real drag to the Finder on Tinker Desk's desktop
  # (features/tools/real-gestures.ts). The Swift app's are AppKit drag
  # sessions, which follow the real pointer, so they can't be scripted there.
  @drag.fileout @transcript.images @swift-pending
  Scenario: A picture in the transcript dragged onto the thread goes into the message
    When I open the thread "Make uploads resume after a crash"
    And I open the line saying "Worked for 21s"
    And I drag picture 1 of the row of 2 pictures onto the transcript
    Then the composer holds 1 picture

  @drag.fileout @highlights.dragout @swift-pending
  Scenario: A file in Highlights dragged onto the thread goes into the message
    When I open the thread "Rebuild the three landing pages"
    And I drag the highlight "pricing-before.png" onto the transcript
    Then the composer holds 1 picture

  @drag.fileout @file.picture @swift-pending
  Scenario: The picture shown in the pane dragged onto the thread goes into the message
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "pricing-before.png"
    And the pane shows a picture
    And I drag the picture in the pane onto the transcript
    Then the composer holds 1 picture

Feature: Dragging pictures and files out
  As someone sharing what an agent made
  I want to drag a picture or a file straight out of the app
  So I can post it or keep it without opening it first

  Background:
    Given the app is connected to a runner with the demo threads

  # In the window it's the app's own drag, which the message box takes as a
  # dropped file; out of the window it's handed to the system as the file
  # itself, tried with a real drag to the Finder on Tinker Desk's desktop
  # (features/tools/real-gestures.ts). Let go anywhere else over the thread,
  # it's a drag out that changed its mind, and the Swift app turns it away
  # (`takesDropFromThread`). The Swift app's are AppKit drag sessions, which
  # follow the real pointer, so they can't be scripted there.
  @drag.fileout @transcript.images @swift-pending
  Scenario: A picture in the transcript dragged onto the message box goes into the message
    When I open the thread "Make uploads resume after a crash"
    And I open the line saying "Worked for 21s"
    And I drag picture 1 of the row of 2 pictures onto the message box
    Then the composer holds 1 picture

  @drag.fileout @transcript.images @swift-pending
  Scenario: A picture dragged out of the transcript and let go over the thread stays out of the message
    When I open the thread "Make uploads resume after a crash"
    And I open the line saying "Worked for 21s"
    And I drag picture 1 of the row of 2 pictures onto the transcript
    Then the composer holds 0 pictures

  @drag.fileout @highlights.dragout @swift-pending
  Scenario: A file in Highlights dragged onto the message box goes into the message
    When I open the thread "Rebuild the three landing pages"
    And I drag the highlight "pricing-before.png" onto the message box
    Then the composer holds 1 picture

  @drag.fileout @file.picture @swift-pending
  Scenario: The picture shown in the pane dragged onto the message box goes into the message
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "pricing-before.png"
    And the pane shows a picture
    And I drag the picture in the pane onto the message box
    Then the composer holds 1 picture

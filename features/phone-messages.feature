Feature: Messages sent from the phone
  As someone using Threadbear on more than one device
  I want to see which messages I sent from my phone
  So I can recognize them in a thread

  Background:
    Given the app is connected to a runner with the demo threads

  @transcript.user @remote-threads
  Scenario: A phone message has its phone mark and a Mac message does not
    When I open the thread "Tidy the media drive"
    Then the phone origin mark is drawn
    When I open the thread "Make uploads resume after a crash"
    Then no phone origin mark is drawn

  @transcript.user @remote-threads
  Scenario: A picture sent without words still has its phone mark
    Given the backup thread has a phone message with only a picture
    When I open the thread "Check the photo library backup"
    Then the phone origin mark is drawn

  # Swift's click shows an AppKit tooltip, outside its scripted view tree.
  @transcript.user @remote-threads @swift-pending
  Scenario: Clicking the phone mark says where the message came from
    When I open the thread "Tidy the media drive"
    And I press the phone origin mark
    Then the phone origin explanation is drawn

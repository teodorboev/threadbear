# Swift has a PDF picture, but its frozen peek command cannot report a
# raster’s dimensions. Video hover pictures are absent from its LinkPeek.
Feature: A document or film on a link's hover card
  As someone checking a file an agent made
  I want its first page or frame on the link's card
  So I can recognize it before opening it

  Background:
    Given the app is connected to a runner with the demo threads

  @rich.linkpeek @swift-pending @windows-pending @linux-pending
  Scenario: A local PDF link shows a small first-page picture
    When I open the thread "Rebuild the three landing pages"
    And a reply links "the document" to the sample file "docs/launch-brief.pdf"
    Then the hover on "the document" shows a raster at most 552 by 340
    And nothing was opened in another app

  @rich.linkpeek @swift-pending @windows-pending @linux-pending
  Scenario: A local video link shows a small first-frame picture
    When I open the thread "Rebuild the three landing pages"
    And a reply links "the film" to the sample file "screens/hero-walkthrough.mp4"
    Then the hover on "the film" shows a raster at most 552 by 340
    And nothing was opened in another app

  @rich.linkpeek @swift-pending @windows-pending @linux-pending
  Scenario: An unreadable PDF keeps its file symbol
    Given a file "empty.pdf" saying "" in a folder of its own
    When I open the thread "Morning brief"
    And a reply linking "the empty PDF" to that file streams in
    Then the hover on "the empty PDF" falls back to the file symbol
    And nothing was opened in another app

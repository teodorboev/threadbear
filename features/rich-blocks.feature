Feature: Diagrams and pages in a reply
  As someone an agent explains things to
  I want a mermaid diagram or a small html page in a reply drawn as what it shows
  So a picture can say what words can't, with its source a click away

  # The Swift app draws these with WebKit; the GPUI app with its web runtime
  # (desktop/web), which @web-runtime scenarios install from the pinned
  # archive first (support/web-runtime.ts), and which only the Mac has so far.

  Background:
    Given the app is connected to a runner with the demo threads

  @rich.mermaid @rich.chrome @web-runtime
  Scenario: A mermaid block is drawn as a diagram, with its source a click away
    Given the web runtime is installed
    When I open the thread "Morning brief"
    And a reply with a mermaid diagram streams in
    Then the reply shows its mermaid block drawn
    When I press "Show the source" on the box
    Then the reply shows its mermaid block's source
    When I press "Show the diagram" on the box
    Then the reply shows its mermaid block drawn

  @rich.chrome @web-runtime
  Scenario: Copy puts a block's source on the clipboard
    Given the web runtime is installed
    When I open the thread "Morning brief"
    And a reply with a mermaid diagram streams in
    Then the reply shows its mermaid block drawn
    When I press "Copy the source" on the box
    Then the clipboard holds the mermaid diagram's source

  @rich.html @web-runtime
  Scenario: An html block is drawn as a page
    Given the web runtime is installed
    When I open the thread "Morning brief"
    And a reply with an html page streams in
    Then the reply shows its html block drawn

  # The Swift app's lockdown is checked by its own tests (the preview egress
  # tests and RichBlockTests); this drives the GPUI app's through the path a
  # reply's page takes.
  @rich.html @swift-pending @web-runtime
  Scenario: An agent's page runs none of its scripts and reaches nothing
    Given the web runtime is installed
    When a page that tries to run scripts and reach the network is drawn
    Then its scripts didn't run and nothing was reached

  @rich.fullsize @web-runtime
  Scenario: A diagram opens full size in a window of its own
    Given the web runtime is installed
    When I open the thread "Morning brief"
    And a reply with a mermaid diagram streams in
    Then the reply shows its mermaid block drawn
    When I press "Open it full size in its own window" on the box
    Then the diagram is open in a window of its own

  @rich.mermaid @rich.html
  Scenario: A block still streaming in stays code until its fence closes
    When I open the thread "Morning brief"
    And a reply with an unfinished mermaid diagram streams in
    Then the reply shows no rich blocks

  # The Swift app has WebKit built in, so it never asks.
  @rich.mermaid @foundation.webruntime @swift-pending @web-prompt
  Scenario: Without the web runtime a diagram shows its source and asks to install it
    When I open the thread "Morning brief"
    And a reply with a mermaid diagram streams in
    Then the reply shows its mermaid block's source, asking about the web runtime
    When I choose Not now on the box
    Then the reply shows its mermaid block's source without asking

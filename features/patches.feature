Feature: What a folder's patches give a thread
  As someone working in a project with its own tools
  I want its chips, panels and the code they run laid out where I can see them
  So I know what's there and can trust it on purpose

  Background:
    Given the app is connected to a runner with the demo threads

  @patches.list
  Scenario: The Patches list says where each patch comes from and what it gives
    When I open the thread "Add dark mode to the dashboard"
    And I show the "threadbear.patches" section of the pane
    Then the patch "tools" comes from "project"
    And the patch "tools" gives "actions Typecheck, Open logs, Deploy to prod, Reseed dev DB"
    And the patch "convex" comes from "built in"

  @patches.list
  Scenario: A project heading in Patches opens the project
    When I open the thread "Add dark mode to the dashboard"
    And I show the "threadbear.patches" section of the pane
    And I follow the project heading in Patches
    Then the patch's project page is open

  @patches.list
  Scenario: A patch that isn't trusted waits, and Trust brings it in
    When I open the thread "Pick a colour for the new Share button"
    And I show the "threadbear.patches" section of the pane
    Then the patch "release" is waiting to be trusted, saying "Not run yet"
    When I trust the patch "release"
    Then the patch "release" gives "action Upload build"

  @panel.list
  Scenario: A list panel draws its rows under their headings
    When I open the thread "Rebuild the three landing pages"
    And I show the "deploys.previews" section of the pane
    Then the list panel has the heading "This branch" with the rows "Home | Pricing | Docs"
    And the list panel has the heading "Elsewhere" with the rows "Local | Production"
    And the list row "Pricing" says "12m" and "Waiting on the plan prices"

  @panel.image
  Scenario: An image panel draws its picture
    When I open the thread "Pick a colour for the new Share button"
    And I show the "xcode.simulator" section of the pane
    Then the image panel shows a picture

  # Swift launches Simulator directly and cannot record this scripted open.
  @panel.image @swift-pending @windows-pending @linux-pending
  Scenario: The simulator picture opens Simulator
    When I open the thread "Pick a colour for the new Share button"
    And I show the "xcode.simulator" section of the pane
    Then the image panel shows a picture
    When I open Simulator from the picture
    Then Simulator was opened through the app

  @manual @panel.web
  Scenario: A url panel's link to another site doesn't take over the panel
    When a patch's url panel shows its page on localhost with a link to a video on another site
    And I click the link
    Then the video opens where Default says, in the browser or over the panel with Back
    And the panel still shows the patch's page
    When I click a link to another of the patch's own pages
    Then the panel's bar has Back, which goes back to the first page

  @manual @panel.web
  Scenario: Closing a url panel stops what it's playing
    When a patch's url panel is playing a video
    And I close the panel's tab
    Then the video stops

  @patches.run
  Scenario: A chip runs its action and it lands in the transcript
    When I open the thread "Add dark mode to the dashboard"
    Then the chip "tools.reseed-dev-db" is offered
    When I run the chip "tools.reseed-dev-db"
    Then the transcript shows a line about "Reseed dev DB"

  @manual @patches.list
  Scenario: A home patch that makes every edit ask says which and why
    When a home patch's panel runs a command built from a value only known as it runs, like "node ${script}"
    And I show the "threadbear.patches" section of the pane
    Then the patch says threads in Ask and Auto ask before every file edit, with the command, its line and what to change
    And an Auto thread's first edit that asks says the same in the transcript

  # Each section of the pane draws its own wherever the dock puts it, as the
  # Swift app's DockPane does; the GPUI pane drew one section for both until
  # 13 Nov. `panels.drawn` is the GPUI dump's own.
  @pane.sections @swift-pending
  Scenario: Highlights beside Files shows both, each its own
    When I open the thread "Rebuild the three landing pages"
    And I show the "threadbear.files" section of the pane
    And I move the "files" panel to the "right" edge
    Then the "threadbear.highlights" section draws "highlights"
    And the "threadbear.files" section draws "files"
    When I show the "threadbear.highlights" section of the pane
    And I click the highlight "copy-review.md"
    Then the "threadbear.highlights" section draws "preview copy-review.md"
    And the "threadbear.files" section draws "files"

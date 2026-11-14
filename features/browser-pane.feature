Feature: The Browser pane
  As someone whose agents use web pages through BrowserBear
  I want each thread's pages listed beside it, with the one handed to me in front
  So I can find the page an agent means, sign in there and tell it I'm done

  Background:
    Given the app is connected to a runner with the demo threads

  # The Swift app's dump has no Browser button or pane (AppDump.rails, Pane),
  # and it's frozen, so these are checked on the GPUI app only.
  #
  # None run on Windows: BrowserBear is a Mac tool (Linux works too) that talks
  # over a Unix socket, which the GPUI app only opens on Unix
  # (agent_browser.rs), and the stand-in can't listen on a file path there
  # (Windows wants a named pipe: listen EACCES on browserbear.sock).

  @browserbear @windows-pending @browser.pages @rail.right @swift-pending
  Scenario: The Browser button counts a thread's pages and the pane lists them
    Given BrowserBear has "Example Domain" at "https://example.com/" open for "Morning brief"
    When I open the thread "Morning brief"
    Then the right rail's Browser button shows 1 page
    When I click the right rail's Browser button
    Then the Browser pane lists "Example Domain" from "example.com" with "Show window"

  @browserbear @windows-pending @browser.show @swift-pending
  Scenario: Show window asks BrowserBear to bring the page's window forward
    Given BrowserBear has "Example Domain" at "https://example.com/" open for "Morning brief"
    When I open the thread "Morning brief"
    And I click the right rail's Browser button
    And I press "Show window" on "Example Domain"
    Then BrowserBear was asked to show that page for "Morning brief"

  @browserbear @windows-pending @browser.handoff @swift-pending
  Scenario: A page handed to me opens the pane by itself, and Done tells the agent
    Given BrowserBear has "Sign in" at "https://login.example.com/" open for "Morning brief"
    When I open the thread "Morning brief"
    And the agent hands "Sign in" to me
    Then the dock has the "browser" panel
    And the Browser pane lists "Sign in" from "login.example.com" with "Show window, Done"
    When I press "Done" on "Sign in"
    Then BrowserBear was told I'm done with "Sign in"
    And the Browser pane lists "Sign in" from "login.example.com" with "Show window"

  @browserbear @windows-pending @browser.handoff @panel.requests @swift-pending
  Scenario: show_panel for the browser brings the Browser pane up
    Given BrowserBear has "Example Domain" at "https://example.com/" open for "Morning brief"
    When I open the thread "Morning brief"
    And I'm looking at the app
    And the agent asks to show the "browser" panel
    Then the dock has the "browser" panel
    And the runner heard the panel was shown

  # The Qantas sign-in (30 Oct): the agent's tab on the page came forward
  # rather than a second one opening in the browser.
  @browserbear @windows-pending @rich.browserbear
  Scenario: A link to a page an agent has open brings its tab forward instead of opening another
    Given BrowserBear has "Sign in" at "https://login.example.com/" open for "Morning brief"
    When I open the thread "Morning brief"
    And a reply linking "sign in here" to "https://login.example.com/" streams in
    And I click the link "sign in here"
    Then BrowserBear was asked to hand over "https://login.example.com/"
    And nothing was opened in another app

  @browserbear @windows-pending @rich.browserbear
  Scenario: A link to a page no agent has open opens in the browser
    Given BrowserBear has "Sign in" at "https://login.example.com/" open for "Morning brief"
    When I open the thread "Morning brief"
    And a reply linking "the docs" to "https://docs.example.com/" streams in
    And I click the link "the docs"
    Then "https://docs.example.com/" was opened in the browser

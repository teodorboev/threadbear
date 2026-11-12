Feature: The web runtime
  As someone who wants diagrams, pages and web panels in the app
  I want Chromium fetched the first time something needs it, and only the build Threadbear pins
  So the app stays small, and what it runs is what it says it runs

  # The Swift app draws pages with WebKit, which comes with macOS, so it has
  # nothing to download, install or check: these are the GPUI app's alone.
  # @web-runtime scenarios run on a Mac given TB_WEB_RUNTIME_ARCHIVE, the
  # pinned build already downloaded (support/world.ts), and are skipped
  # elsewhere; Windows and Linux have no runtime yet.

  Background:
    Given the app is connected to a runner with the demo threads

  @foundation.webruntime @swift-pending @web-runtime @web-runtime-fresh
  Scenario: Installing the web runtime, then drawing a page with it
    Then the web runtime isn't installed
    When I install the web runtime
    Then the web runtime says it's installed
    And the web runtime draws a page with no window

  @foundation.webruntime @swift-pending @web-prompt
  Scenario: A download that isn't the pinned build is never unpacked
    When I install the web runtime from a file that isn't the pinned build
    Then the web runtime says the download didn't match the pinned build
    And the web runtime isn't installed

  @foundation.webruntime @swift-pending @web-prompt
  Scenario: Settings says where the web runtime has got to
    When I open Settings
    Then Settings says the web runtime is "Not installed"

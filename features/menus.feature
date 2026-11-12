Feature: The menu bar's standard menus
  As someone who expects a Mac app's menus
  I want Edit, File > Close, Window and About where every Mac app has them
  So the system's text tools and window commands are where I look

  Background:
    Given the app is connected to a runner with the demo threads

  @menu.edit
  Scenario: The Edit menu has the text commands
    Then the "Edit" menu lists "Undo | Redo | Cut | Copy | Paste | Delete | Select All"

  # AppKit adds these to a Mac app's Edit menu by itself.
  @menu.edit @windows-pending @linux-pending
  Scenario: The Edit menu has the system's dictation and emoji
    Then the "Edit" menu lists "Start Dictation… | Emoji & Symbols"

  # A scripted Swift app is never the key window, so its menu bar's Select
  # All and Cut, which go to the key window's text view, reach nothing there.
  @menu.edit @swift-pending
  Scenario: Select All, Cut and Paste from the Edit menu work in the message box
    Given I open the thread "Morning brief"
    And I type "Ship it on Friday" in the composer
    When I choose "Select All" from the menu bar
    And I choose "Cut" from the menu bar
    Then the composer is empty
    When I choose "Paste" from the menu bar
    Then the composer says "Ship it on Friday"

  @menu.file
  Scenario: The File menu can close the window
    Then the "File" menu lists "New Thread | Close"

  @menu.window
  Scenario: The Window menu has Minimize, Zoom and Bring All to Front
    Then the "Window" menu lists "Minimize | Zoom | Bring All to Front"

  # The Mac's own keys. AppKit adds View's Enter Full Screen and Window's
  # Fill, Center and Move & Resize only as the menu opens, which neither
  # app's script can make it do, so those are seen in the real menu bar.
  @menu.file @menu.window @windows-pending @linux-pending
  Scenario: The window commands have the Mac's keys
    Then the "File" menu has "Close" on "⌘W"
    And the "Window" menu has "Minimize" on "⌘M"

  @menu.window @windows-pending @linux-pending
  Scenario: A panel in a window of its own is listed in the Window menu
    Given I open the thread "Morning brief"
    When I put the "highlights" panel in a window of its own
    Then the "Window" menu lists "Highlights"

  # A scripted app's window is never in front, and closing it ends the run,
  # so this is tried by hand on Tinker Desk's desktop.
  @menu.file @manual
  Scenario: Closing the main window quits the app, as ⌘W does
    When I close the main window with ⌘W
    Then the app quits without asking

  @tour.about
  Scenario: About Threadbear opens a window of its own
    When I choose "About Threadbear" in the app menu
    Then the About window is open

  # The GPUI app reads its About window's lines back; the Swift command only
  # says the window's up.
  @tour.about @swift-pending
  Scenario: The About window says what the app knows and who made it
    When I choose "About Threadbear" in the app menu
    Then the About window says "Threadbear"
    And the About window says "Made in Busselton by Mike and a lot of bears."

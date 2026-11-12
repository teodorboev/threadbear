Feature: A folder in the side pane
  As someone looking at what's on disk beside a thread
  I want a Finder-like list of the thread's folder
  So I can find a file without leaving the app

  Background:
    Given the app is connected to a runner with the demo threads

  @folder.filespanel @folder.list
  Scenario: The Files panel lists the thread's folder, folders first
    When I open the thread "Rebuild the three landing pages"
    And I show the "threadbear.files" section of the pane
    Then the folder lists "docs, logs, mockups, screens, src" first
    And the folder says it's sorted by "name up"

  @folder.list
  Scenario: Sorting by name the other way turns the list round
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I sort the folder by "name" descending
    Then the folder lists "src, screens, mockups, logs, docs" first
    And the folder says it's sorted by "name down"

  @folder.list
  Scenario: Expanding a folder lists its rows under it
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I expand the folder row "logs"
    Then the folder shows 6 rows in all

  @folder.list
  Scenario: A double-click goes into a folder, and Back comes back
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I double-click the folder row "logs"
    Then the pane is browsing the folder "logs"
    And the folder lists "docs-build.txt" first
    When I press Back in the pane
    Then the pane shows the thread's folder again

  @folder.pathbar
  Scenario: The path bar names each folder down to this one
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I double-click the folder row "logs"
    Then the path bar ends with "site > logs"

  @folder.menu
  Scenario: A row's right-click menu offers what can be done with it
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I right-click the folder row "docs"
    Then the folder menu offers "Open | Open in Finder | Reveal in Finder | Copy path | --- | Show as icons | Sort by | Show hidden files"

  @folder.list @foundation.filewatch
  Scenario: A file made in the open folder shows up, and one taken away goes
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When a file "notes.txt" is made in the thread's folder
    Then the folder lists "notes.txt"
    When the file "notes.txt" is taken out of the thread's folder
    Then the folder no longer lists "notes.txt"

  @folder.list @folder.menu
  Scenario: More than one row can be picked, and the menu then copies all their paths
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I pick the folder row "docs"
    And I add the folder row "logs" to the picked rows
    Then the folder has "docs + logs" picked
    When I right-click the folder row "logs"
    Then the folder menu offers "Open | Copy paths | Show as icons"

  @folder.keys
  Scenario: Return opens the picked row, and ⌘↑ comes back up with it picked
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I pick the folder row "logs"
    And I press "return" in the folder
    Then the pane is browsing the folder "logs"
    When I press "cmd-up" in the folder
    Then the pane shows the thread's folder again
    And the folder has "logs" picked

  @folder.keys
  Scenario: ⌘⇧. shows the hidden files and hides them again
    Given a hidden file ".env" is in the folder of "Rebuild the three landing pages"
    And the thread's folder is showing in the pane for "Rebuild the three landing pages"
    Then the folder doesn't list ".env"
    When I press "cmd-shift-." in the folder
    Then the folder lists ".env"
    When I press "cmd-shift-." in the folder
    Then the folder doesn't list ".env"

  @folder.keys
  Scenario: Typing a name picks its row, and Space looks at what's picked
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I type "sc" in the folder
    Then the folder has "screens" picked
    And Space would look at "screens"

  @folder.icons
  Scenario: Show as icons draws the folder as tiles, and what's picked carries across
    Given the thread's folder is showing in the pane for "Rebuild the three landing pages"
    When I choose "Show as icons" from the folder's menu
    Then the folder is showing as "icons"
    When I pick the folder row "screens"
    And I choose "Show as a list" from the folder's menu
    Then the folder is showing as "list"
    And the folder has "screens" picked

  @folder.pathbar
  Scenario: The path bar's … is a menu of the folders above the ones it shows
    Given the thread "Rebuild the three landing pages" works in a folder too deep for the path bar to show whole
    And the thread's folder is showing in the pane for "Rebuild the three landing pages"
    Then the path bar's … menu starts with the folder above the first one it shows
    When I pick the last folder in the path bar's … menu
    Then the pane is browsing that folder

Feature: Working on a file in the side pane
  As someone fixing what an agent wrote
  I want to type into a file beside the conversation, saved as I go
  So a small fix doesn't need another app, and nobody's work is lost

  Background:
    Given the app is connected to a runner with the demo threads

  @file.editor
  Scenario: The pencil opens the file's text to type in, and what's typed is saved by itself
    When I open the thread "Rebuild the three landing pages"
    And I open "logs/docs-build.txt" in the thread's folder in the pane
    Then the pane's file can be edited
    When I start editing the pane's file
    Then the pane's file is being edited
    When I type "Fixed in 4b7d0a8." at the end of the pane's file
    Then the file "logs/docs-build.txt" on disk ends with "Fixed in 4b7d0a8."
    When I'm done editing the pane's file
    Then the pane's file can be edited
    And the pane shows a "text" page with the words "Fixed in 4b7d0a8."

  @file.editor @file.markdown
  Scenario: ⌘S saves at once, in a Markdown file's own text
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    And I pick "markdown" from the pane's Markdown menu
    And I type "- Footer links checked." at the end of the pane's file, without waiting
    And I save the pane's file
    Then the file "docs/copy-review.md" on disk ends with "- Footer links checked."

  @file.conflict
  Scenario: A file changed on disk under unsaved typing asks which to keep, and Keep mine writes mine
    When I open the thread "Rebuild the three landing pages"
    And I open "src/hero.ts" in the thread's folder in the pane
    And I start editing the pane's file
    And I type "// mine" at the end of the pane's file, without waiting
    And "src/hero.ts" in the thread's folder is rewritten as "// theirs"
    Then the pane says "Changed on disk while you were editing."
    When I keep mine in the pane
    Then the file "src/hero.ts" on disk ends with "// mine"

  @file.conflict
  Scenario: Taking the file's side shows what's on disk, and writes nothing
    When I open the thread "Rebuild the three landing pages"
    And I open "src/hero.ts" in the thread's folder in the pane
    And I start editing the pane's file
    And I type "// mine" at the end of the pane's file, without waiting
    And "src/hero.ts" in the thread's folder is rewritten as "// theirs"
    Then the pane says "Changed on disk while you were editing."
    When I take the file's side in the pane
    Then the pane's file reads "// theirs"
    And the file "src/hero.ts" on disk ends with "// theirs"

  @foundation.filewatch @file.textview
  Scenario: A file an agent rewrites shows its new words by itself
    When I open the thread "Rebuild the three landing pages"
    And I open "logs/docs-build.txt" in the thread's folder in the pane
    Then the pane shows a "text" page with the words "Could not resolve image"
    When "logs/docs-build.txt" in the thread's folder is rewritten as "Build passed in 9.8s."
    Then the pane shows a "text" page with the words "Build passed in 9.8s."

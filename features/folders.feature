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

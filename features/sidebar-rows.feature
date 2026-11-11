Feature: What a thread's row says in the sidebar
  As someone scanning a long list of threads
  I want each row to say what the thread is and what it's doing
  So I can tell at a glance which one to open

  Background:
    Given the app is connected to a runner with the demo threads

  @sidebar.row
  Scenario: A row shows its project and where it lives
    Then the row "Make uploads resume after a crash" has the tile "P"
    And the row "Make uploads resume after a crash" is in "Photo sync"
    And the row "Morning brief" has the tile "folder"
    And the row "Morning brief" is in "misc"

  @sidebar.subagents
  Scenario: A running thread lists its sub-agents under it, nested ones indented
    Then the row "Why is the build slow on CI?" lists the sub-agent "Find the commit that slowed the build"
    And the row "Why is the build slow on CI?" lists the sub-agent "Time the test step on each commit" indented by 1
    And the sub-agent "Check the Docker layer cache" of "Why is the build slow on CI?" says "Failed: Couldn't reach the registry."

  @sidebar.row.origin
  Scenario: A thread someone else started says so before its place line
    Then the row "Rename the Settings tabs" has the origin mark "pawprint"
    And the row "Make uploads resume after a crash" has no origin mark

  @sidebar.row.marks
  Scenario: A thread that needs me has a blue dot and a bold title
    Then the row "Make uploads resume after a crash" has a blue dot
    And the row "Make uploads resume after a crash" has a bold title

  @sidebar.row.marks
  Scenario: A finished thread I haven't read has a tick
    Then the row "Tidy the imports in the uploader" has a tick
    And the row "Tidy the imports in the uploader" has a bold title

  @sidebar.row.marks
  Scenario: A thread I've read has no mark and a plain title
    Then the row "Rename the Settings tabs" has no mark
    And the row "Rename the Settings tabs" has a plain title

  @sidebar.row.marks
  Scenario: Opening a finished thread takes its tick away
    Given the window is in front
    And the row "Tidy the imports in the uploader" has a tick
    When I open the thread "Tidy the imports in the uploader"
    Then the row "Tidy the imports in the uploader" has no mark

  @sidebar.row.marks
  Scenario: Opening a thread doesn't move it in the Inbox
    Given the window is in front
    And I note where "Tidy the imports in the uploader" is in the "Inbox" section
    When I open the thread "Tidy the imports in the uploader"
    Then the row "Tidy the imports in the uploader" has no mark
    And "Tidy the imports in the uploader" is still in the same place in the "Inbox" section

  @sidebar.row.status
  Scenario: A turn under way shows how long it has been going
    Then the row "Why is the build slow on CI?" shows a running timer

  @sidebar.row.status
  Scenario: A starred thread with a turn under way says Working
    When I star the thread "Why is the build slow on CI?"
    Then the "Starred" section lists "Why is the build slow on CI?"
    And the row "Why is the build slow on CI?" says "Working" on the right

  @sidebar.row.status
  Scenario: Background work says what's still running
    Then the row "Rebuild the three landing pages" says "3 sub-agents" on the right

  @sidebar.row.status
  Scenario: A failed turn says Failed
    Then the row "Make a poster for the meetup" says "Failed" on the right

  @sidebar.row.status
  Scenario: A thread with no login free says when it carries on
    Then the row "Draft the release notes for 0.4" says "Waiting for room" on the right

  @sidebar.row.status
  Scenario: A hibernating thread says when it's back
    When I hibernate the thread "Morning brief" for 1 hour
    Then the "Hibernating" section lists "Morning brief"
    And the row "Morning brief" says "Back" on the right

  @sidebar.row.status
  Scenario: A thread that woke up by itself says so
    When I hibernate the thread "Morning brief" for 1 second
    Then the row "Morning brief" says "Woke up" on the right

  @sidebar.row.status
  Scenario: Other rows say how long ago they were last active
    Then the row "Rename the Settings tabs" shows when it was last active

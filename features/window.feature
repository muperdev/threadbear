Feature: The window and its panels
  As someone who keeps one window open all day
  I want the panels, the zoom and the folds to stay as I left them
  So the window is always how I like it

  Background:
    Given the app is connected to a runner with the demo threads

  @dock.layout
  Scenario: The window starts with the threads, the thread and Highlights
    Then the panels are laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"

  @dock.reset
  Scenario: Reset Panel Layout puts moved panels back
    When I move the "highlights" panel to the "left" edge
    Then the panels aren't laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"
    When I choose "Reset Panel Layout" from the menu bar
    Then the panels are laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"

  @dock.layout
  Scenario: The panel layout is remembered when the app is opened again
    When I move the "highlights" panel to the "left" edge
    And the app is quit and opened again
    Then the panels aren't laid out as "row[threads (no title) 29% | thread 47% | highlights 24%]"

  # Scripted runs place the window for their own ends and never save it, in
  # both apps (MainWindowFrame.scripted), so this one needs checking by hand.
  # Nobody has yet on the GPUI app: only the on-screen rule and the file have tests.
  @dock.frame @manual
  Scenario: The window reopens where it was left
    When I move the window and quit the app
    And I open the app again
    Then the window is where it was

  # A real drag through the window server, which a scripted click can't make:
  # features/tools/window-drag.ts does this from each part of the top of the
  # window on Tinker Desk's desktop (it takes the pointer, so never on Mike's).
  @dock.titlebar @manual
  Scenario: Dragging the top of the window moves it, and a click on the title still renames
    When I drag the window by the bar over the thread, by its title and beside it
    Then the window moves with the pointer each time
    And a click on the title starts renaming the thread

  @foundation.zoom
  Scenario: Zooming makes everything bigger, and Actual Size puts it back
    Given I open the thread "Morning brief"
    And I note how tall a row on screen is
    When I press "=" with "cmd"
    Then the zoom is 110 percent
    And that row is taller than it was
    When I choose Actual Size
    Then the zoom is 100 percent

  # ⌘0 itself is GPUI-only: a scripted Swift app's Actual Size item keeps its
  # greyed-out state from the last time the menu was opened.
  @foundation.zoom @swift-pending
  Scenario: ⌘0 puts the zoom back to Actual Size
    When I set the zoom to 125 percent
    And I press "0" with "cmd"
    Then the zoom is 100 percent

  @foundation.zoom
  Scenario: The zoom is remembered when the app is opened again
    When I set the zoom to 125 percent
    And the app is quit and opened again
    Then the zoom is 125 percent

  @sidebar.sections
  Scenario: Folded sections are remembered when the app is opened again
    When I click the "Inbox" heading
    Then the "Inbox" section is folded
    When the app is quit and opened again
    Then the "Inbox" section is folded

  @sidebar.sort
  Scenario: The Inbox's sort is remembered when the app is opened again
    When I sort the Inbox oldest first
    And the app is quit and opened again
    Then the Inbox is sorted oldest first

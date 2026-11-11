Feature: The thread's bar, banners and notes
  As someone working through threads
  I want the title and the thread's actions in its bar, and clear word when something's wrong
  So I always know what's happening

  Background:
    Given the app is connected to a runner with the demo threads

  @header.title
  Scenario: The panel's bar shows the open thread's title
    When I open the thread "Morning brief"
    Then the panel bar's title says "Morning brief"

  @header.title
  Scenario: Clicking the title renames the thread
    When I open the thread "Morning brief"
    And I start renaming the open thread
    Then the panel bar's title is a field
    When I rename the open thread to "Evening brief"
    Then the panel bar's title says "Evening brief"
    And the sidebar lists "Evening brief"

  # The demo's "Draft the release notes" is on the Evenings pool, whose only
  # login is at its limit, so the runner is holding it until there's room.
  @banner.limit
  Scenario: A pool thread out of room says when it carries on, and offers a login outside the pool
    When I open the thread "Draft the release notes for 0.4"
    Then the limit line starts "Every login in Evenings is out of room. It carries on"
    And the limit line's button says "Pin to mike (78% left)"
    And the model chip is marked "Limit reached"

  @header.backforward
  Scenario: The back and forward buttons say where they go
    When I open the thread "Morning brief"
    And I open the thread "Rename the Settings tabs"
    Then the back button's tip says "Back to Morning brief (⌘[)"
    And the forward button's tip says "Nothing to go forward to (⌘])"

  @header.backforward
  Scenario: Clicking back opens the thread before it
    When I open the thread "Morning brief"
    And I open the thread "Rename the Settings tabs"
    And I click the back button
    Then the open thread is "Morning brief"

  @header.more
  Scenario: The ... menu copies the thread's id
    When I open the thread "Morning brief"
    And I choose Copy thread id from the ... menu
    Then the clipboard holds the open thread's id

  @header.share
  Scenario: Sharing a thread makes a link for it
    When I open the thread "Morning brief"
    And I make a share link for the open thread
    Then the share popover says the thread is shared with a link

  @banner.error
  Scenario: An error shows in a bar until it's dismissed
    When an error "Thread not found" happens
    Then the error bar says "Thread not found"
    When I dismiss the error bar
    Then there's no error bar

  @banner.connection
  Scenario: Losing the runner shows a banner until it's back
    When I open the thread "Morning brief"
    And the runner restarts
    Then the banner says the runner is restarting or reconnecting
    And there's no banner once the runner is back

  @toast.archived
  Scenario: Archiving a thread shows a note with Undo
    When I open the thread "Morning brief"
    And I archive the open thread
    Then the Archived note names "Morning brief"
    When I press Undo on the Archived note
    Then there's no Archived note
    And the "Inbox" section lists "Morning brief"
    And the open thread is "Morning brief"

  @toast.archived
  Scenario: Clicking the Archived note opens the thread in the Archived section
    When I open the thread "Morning brief"
    And I archive the open thread
    And I click the Archived note
    Then the open thread is "Morning brief"
    And the "Archived" section lists "Morning brief"

  @toast.inboxend
  Scenario: Next Inbox Thread says when the inbox has run out
    When I open the last thread in the Inbox
    And I press "down" with "cmd,opt"
    Then a note says I've reached the end of the inbox

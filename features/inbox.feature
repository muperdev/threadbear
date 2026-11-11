Feature: The inbox
  As someone running a lot of agent threads
  I want them sorted by what they need from me
  So I can see at a glance what to look at next

  Background:
    Given the app is connected to a runner with the demo threads

  @sidebar.sections
  Scenario: Threads are grouped by what's happening with them
    Then the "Inbox" section lists "Make uploads resume after a crash"
    And the "Running" section lists "Why is the build slow on CI?"
    And the "Archived" section lists "Write the Quick Ask README"
    And the "Archived" section is folded

  @sidebar.row.marks
  Scenario: Threads that need me come before the ones that don't
    Then "Make uploads resume after a crash" comes before "Rename the Settings tabs" in the "Inbox" section
    And "Make uploads resume after a crash" needs me
    And "Rename the Settings tabs" doesn't need me

  @sidebar.row.marks
  Scenario: Opening a thread marks it as read
    Given the window is in front
    And "Tidy the imports in the uploader" is unread
    When I open the thread "Tidy the imports in the uploader"
    Then "Tidy the imports in the uploader" is no longer unread

  @sidebar.search
  Scenario: Searching narrows the list to the threads that match
    When I search the threads for "poster"
    Then the sidebar lists "Make a poster for the meetup"
    And the sidebar doesn't list "Morning brief"

  @sidebar.search
  Scenario: Clearing the search brings every thread back
    Given I search the threads for "poster"
    When I clear the search
    Then the sidebar lists "Morning brief"

  @sidebar.row.hoverbuttons
  Scenario: Archiving a thread takes it out of the inbox
    When I open the thread "Morning brief"
    And I archive the open thread
    Then the "Inbox" section doesn't list "Morning brief"
    And the "Archived" section lists "Morning brief"

  @sidebar.hibernate.presets
  Scenario: Hibernating a thread puts it away for a while
    When I open the thread "Morning brief"
    And I hibernate the open thread for an hour
    Then the "Inbox" section doesn't list "Morning brief"
    And the "Hibernating" section lists "Morning brief"

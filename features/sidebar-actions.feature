Feature: Acting on a thread from its sidebar row
  As someone working through the inbox
  I want to star, hibernate, archive and otherwise handle a thread right there
  So I don't have to open each one to deal with it

  Background:
    Given the app is connected to a runner with the demo threads

  @sidebar.row.hoverbuttons
  Scenario: Hovering a row swaps its time for its buttons
    When I hover the row "Morning brief"
    Then the row "Morning brief" offers the buttons "Star: keep at the top, Hibernate, Archive"

  @sidebar.row.hoverbuttons
  Scenario: Starring from the row's button moves it to Starred at once
    When I hover the row "Morning brief"
    And I press the row button "Star: keep at the top"
    Then the "Starred" section lists "Morning brief"
    And the "Inbox" section doesn't list "Morning brief"

  @sidebar.row.hoverbuttons
  Scenario: Archiving from the row's button takes it out of the inbox
    When I hover the row "Morning brief"
    And I press the row button "Archive"
    Then the "Inbox" section doesn't list "Morning brief"
    And the "Archived" section lists "Morning brief"

  @sidebar.row.hoverbuttons
  Scenario: An archived row offers Unarchive and Delete, and Unarchive brings it back
    Given the "archived" section is unfolded
    And the "inbox" section is folded away
    When I hover the row "Write the Quick Ask README"
    Then the row "Write the Quick Ask README" offers the buttons "Unarchive, Delete chat…"
    When I press the row button "Unarchive"
    Then the "Archived" section doesn't list "Write the Quick Ask README"

  @sidebar.newthread
  Scenario: The compose button leaves the open thread for a new one
    Given I open the thread "Morning brief"
    When I press the row button "Start a new thread"
    Then no thread is open

  @sidebar.row.menu
  Scenario: Right-clicking a row offers what can be done to it
    When I right-click the row "Morning brief"
    Then the row's menu offers "Rename | Star | Hibernate | Archive | Copy thread id"

  @sidebar.hibernate.presets
  Scenario: The Hibernate menu lists when to bring it back
    When I open the Hibernate menu for "Morning brief"
    Then the menu lists "1 hour | Tomorrow, 9 am | Next Monday, 9 am | Pick a time… | Wake when…"

  @sidebar.hibernate.presets
  Scenario: While a restart is waiting, the menu offers coming back after it
    Given a runner restart is waiting
    When I open the Hibernate menu for "Morning brief"
    Then the menu lists "Until the runner restarts"

  @sidebar.hibernate.presets
  Scenario: Choosing a preset hibernates the thread
    When I open the Hibernate menu for "Morning brief"
    And I choose "1 hour" in the menu
    Then the "Inbox" section doesn't list "Morning brief"
    And the "Hibernating" section lists "Morning brief"

  # The calendar and the alert around it have no way in from a script on
  # either app; the GPUI one is checked by a picture for now.
  @manual @sidebar.hibernate.pick
  Scenario: Pick a time hibernates the thread until a date and time
    When I open the Hibernate menu for "Morning brief"
    And I choose "Pick a time…" in the menu
    And I pick tomorrow at 9:00
    Then the "Hibernating" section lists "Morning brief"

  @sidebar.hibernate.wake
  Scenario: Wake when puts it away in my own words
    When I ask to wake the thread "Morning brief" "next time I talk about woodworking"
    Then the "Hibernating" section lists "Morning brief"
    And the row "Morning brief" says "woodworking" on the right

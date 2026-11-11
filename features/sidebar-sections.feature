Feature: Section headings in the sidebar
  As someone with a long list of threads
  I want to fold sections away and choose the Inbox's order
  So I can keep what I'm working through in view

  Background:
    Given the app is connected to a runner with the demo threads

  @sidebar.sections
  Scenario: A folded heading says how many threads it's hiding
    Then the "Archived" section is folded
    And the "Archived" heading says "3"

  @sidebar.sections
  Scenario: An open heading doesn't count its threads
    Then the "Inbox" section isn't folded
    And the "Inbox" heading says nothing

  @sidebar.sections
  Scenario: Clicking a heading folds it, and clicking again unfolds it
    When I click the "Inbox" heading
    Then the "Inbox" section is folded
    When I click the "Inbox" heading
    Then the "Inbox" section isn't folded

  @sidebar.archived.paging
  Scenario: The Archived section draws the most recent few, and more on asking
    Given there are 30 more archived threads
    And the "archived" section is unfolded
    Then the Archived section draws 20 of its 33 threads
    When I ask for more of the Archived section
    Then the Archived section draws 33 of its 33 threads

  @sidebar.sort
  Scenario: The Inbox can be sorted oldest first
    Given the Inbox is sorted by what needs me first
    And "Ship the beta to TestFlight" comes before "Add dark mode to the dashboard" in the "Inbox" section
    When I sort the Inbox oldest first
    Then the Inbox is sorted oldest first
    And "Add dark mode to the dashboard" comes before "Ship the beta to TestFlight" in the "Inbox" section

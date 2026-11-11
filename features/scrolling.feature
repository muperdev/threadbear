Feature: The transcript's scrolling
  As someone reading a long thread while the agent keeps going
  I want the list to stay where I put it and follow along when I'm at the end
  So nothing jumps away from what I'm reading

  Background:
    Given the app is connected to a runner with the demo threads

  @transcript.list
  Scenario: A thread opens at its newest rows
    When I open the thread "Make uploads resume after a crash"
    Then the view is at the bottom

  @transcript.list
  Scenario: While a reply streams in, a view at the bottom stays at the bottom
    When I open the thread "Add dark mode to the dashboard"
    And a reply starts streaming in
    Then the view stays at the bottom

  @transcript.list @long-thread
  Scenario: Reaching the top of a long thread brings older rows in above
    When I open the thread "A very long thread"
    Then there are older rows to page in
    When I scroll up to the first row
    Then older rows arrive above

  @transcript.list @long-thread
  Scenario: Older rows arriving leaves what's on screen where it was
    When I open the thread "A very long thread"
    And I scroll up to the first row and read the rows on screen
    Then older rows arrive above
    And what I was reading is still on screen

  @transcript.list @long-thread
  Scenario: Coming back to a thread returns to where I left it
    When I open the thread "A very long thread"
    And I scroll up to the first row and read the rows on screen
    And I open the thread "Add dark mode to the dashboard"
    And I open the thread "A very long thread"
    Then the view isn't at the bottom
    And what I was reading is still on screen

  @transcript.list @long-thread
  Scenario: A streaming reply doesn't pull a reader away from where they are
    When I open the thread "A very long thread"
    And I scroll up to the first row and read the rows on screen
    And a reply starts streaming in
    Then the view isn't at the bottom
    And what I was reading is still on screen

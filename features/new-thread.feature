Feature: The new-thread page
  As someone with something new to work on
  I want a clear place to say what it is and see where it will start
  So a thread starts where I meant, on a login with room

  Background:
    Given the app is connected to a runner with the demo threads

  @thread.newpage
  Scenario: It asks what to work on and says where the thread will start
    When I start a new thread
    Then the new-thread page asks "What should we work on?"
    And the note under the box says "Starts in misc on the Claude pool"
    And no thread is open

  @thread.newpage
  Scenario: Command-N opens it from inside a thread
    Given I open the thread "Morning brief"
    When I press Command-N
    Then no thread is open
    And the new-thread page asks "What should we work on?"

  @thread.newpage
  Scenario: Each login in the pool shows the room it has left
    When I start a new thread
    Then the room cards say "mike: week 78% left | mike+3: 5h 12% · week 40% left | mike+4: Limit reached"

  @thread.newpage
  Scenario: A card pins the draft to that login
    When I start a new thread
    And I pin the new thread to the login "claude-3"
    Then the note under the box says "Starts in misc on mike+3"
    And the room card "mike+3" is ringed

  @thread.newpage
  Scenario: Choosing a folder changes where it starts
    When I start a new thread
    And I choose the folder "/Users/mike/dev/me/threadbear" for the new thread
    Then the note under the box says "Starts in threadbear on the Claude pool"

  @thread.newpage
  Scenario: The permission chip sets how the new thread runs
    When I start a new thread
    Then the "permission" menu shows "Full access" ticked
    When I pick "Ask before edits" from the "permission" menu
    Then the composer's permission mode is "ask"
    And the "permission" menu shows "Ask before edits" ticked

  @thread.newpage
  Scenario: The model chip on the page sets the model the thread starts on
    When I start a new thread
    Then the model chip says "Claude · Fable 5.1"
    When I pick "Opus 5.5" from the "model" menu
    Then the model chip says "Claude · Opus 5.5"

  @composer.starting
  Scenario: Sending keeps the words, greyed and locked, until the thread exists
    Given the runner takes its time to start threads
    When I start a new thread
    And I type "Find out why the upload stalls" in the composer
    And I press Return
    Then the box is starting a thread with "Find out why the upload stalls"
    And a thread called "Find out why the upload stalls" opens
    And the transcript shows my message "Find out why the upload stalls"
    And the composer is empty

  @thread.newpage
  Scenario: A thread starts with what was picked on the page
    When I start a new thread
    And I choose the folder "/Users/mike/dev/me/threadbear" for the new thread
    And I send "Check the landing copy"
    Then a thread called "Check the landing copy" opens
    And the thread's folder is "/Users/mike/dev/me/threadbear"

  @thread.newpage
  Scenario: After sending, the next new thread starts from the defaults again
    When I start a new thread
    And I choose the folder "/Users/mike/dev/me/threadbear" for the new thread
    And I send "Check the landing copy"
    Then a thread called "Check the landing copy" opens
    When I start a new thread
    Then the note under the box says "Starts in misc on the Claude pool"
    And the composer is empty

  @thread.newpage
  Scenario: A project's New Thread starts in its folder and joins it
    When I start a new thread in the project "p_7a2c41f0"
    Then the new-thread page names the project "In Photo sync"
    And the note under the box says "Starts in photo-sync on the Claude pool"
    When I send "Why did last night's backup fail"
    Then a thread called "Why did last night's backup fail" opens
    And the row "Why did last night's backup fail" says it's in "Photo sync"

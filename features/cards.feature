Feature: Answering the agent
  As someone an agent is waiting on
  I want its questions and requests in front of me with the answers ready to press
  So I can unblock it in a click

  Background:
    Given the app is connected to a runner with the demo threads

  @transcript.approval
  Scenario: An approval shows exactly what it wants to run
    When I open the thread "Make uploads resume after a crash"
    Then an approval card asks to run "rm -rf .build DerivedData"

  @transcript.approval @transcript.approval.settled
  Scenario: Allowing it lets the agent carry on
    When I open the thread "Make uploads resume after a crash"
    And I press "Allow"
    Then the approval card has gone
    And the transcript shows a reply starting "Done"

  @transcript.approval
  Scenario: Denying it stops that action
    When I open the thread "Make uploads resume after a crash"
    And I press "Deny"
    Then the approval for "rm -rf .build DerivedData" says it was denied
    And the transcript shows a reply starting "OK, I left it alone"

  @transcript.question
  Scenario: A question card offers its answers
    When I open the thread "Pick a colour for the new Share button"
    Then a question card asks "Which colour should the Share button be?"

  @transcript.question
  Scenario: Picking an answer and sending it settles the card
    When I open the thread "Pick a colour for the new Share button"
    And I pick the answer "Accent blue" on the question card
    And I send my answer
    Then the transcript shows a reply starting "Going with Accent blue"

  @card.needsyou @card.answers
  Scenario: A turn that needs me shows what it wants decided
    When I open the thread "Ship the beta to TestFlight"
    Then the thread ends with a card of things for me to decide

  @transcript.approval.stacked
  Scenario: Queued calls stack on one card, each with its own answer
    When I open the thread "Add dark mode to the dashboard"
    And the agent queues up several edits
    Then a card asks to allow the 4 queued calls
    When I press "Allow 4"
    Then the transcript shows a reply starting "Allowed 4 of 4"

  @transcript.approval.stacked
  Scenario: Denying all of them denies every one
    When I open the thread "Add dark mode to the dashboard"
    And the agent queues up several edits
    And I press "Deny all"
    Then the transcript shows a reply starting "Allowed 0 of 4"

  @transcript.secret
  Scenario: A secret card can be declined, and says so afterwards
    When I open the thread "Stop Keep Awake asking for my password"
    Then a secret card asks for "Mac administrator password"
    When I press "Decline"
    Then the transcript shows a reply starting "OK, I left it as it is"
    When I open the last folded work line
    Then the transcript shows a step starting "Mac administrator password · Declined"

  @transcript.review
  Scenario: A review card says what it's comparing
    When I open the thread "Compare account picker designs"
    Then a review card says "images · pending"

  @manual @gpui-pending @cards.run
  Scenario: A command on the card runs once and the agent hears how it went
    When I open a thread whose card carries a command
    Then the card shows the whole command and the folder it runs in
    When I press Run
    Then the button shows it's running and can't be pressed again
    And the agent is told the output and carries on

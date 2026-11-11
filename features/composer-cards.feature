Feature: The open card in the composer
  As someone an agent has finished for, or is waiting on
  I want its asks and follow-ups in the box I answer in
  So answering is one move, not a hunt through the transcript

  Background:
    Given the app is connected to a runner with the demo threads

  @card.needsyou
  Scenario: A card of asks sits in the composer
    When I open the thread "Ship the beta to TestFlight"
    Then the card in the composer says "Needs you"
    And the card in the composer asks "1. Accept the new agreement in App Store Connect"
    And the card in the composer asks "2. Check who gets the beta"

  @card.answers
  Scenario: A picked answer waits, then goes with the message
    When I open the thread "Ship the beta to TestFlight"
    And I pick answer 1 of ask 1 on the card
    Then ask 1 on the card is answered "done"
    When I send my picks
    Then the transcript shows my message "1. Accept the new agreement in App Store Connect"

  @card.somethingelse @card.answers
  Scenario: Something else takes my own words for an ask
    When I open the thread "Ship the beta to TestFlight"
    And I write "Just the internal testers" as my own answer to ask 2
    Then ask 2 on the card is answered "something else: \"Just the internal testers\""
    When I send my picks
    Then the transcript shows my message "2. Check who gets the beta"

  @card.returnflow
  Scenario: Return with picks and an ask still open goes no further
    When I open the thread "Ship the beta to TestFlight"
    And I pick answer 1 of ask 1 on the card
    And I press Return
    Then the card in the composer asks "1. Accept the new agreement in App Store Connect"
    And the transcript doesn't show my message "1. Accept the new agreement in App Store Connect"

  @card.returnflow
  Scenario: Return with every ask answered sends them
    When I open the thread "Ship the beta to TestFlight"
    And I pick answer 1 of ask 1 on the card
    And I write "Just the internal testers" as my own answer to ask 2
    And I press Return
    Then the transcript shows my message "1. Accept the new agreement in App Store Connect"

  @card.collapse
  Scenario: The card can fold to its heading
    When I open the thread "Ship the beta to TestFlight"
    And I press "collapse" on the card
    Then the card in the composer says "Needs you (collapsed)"

  @card.done
  Scenario: A done card offers the agent's follow-ups
    When I open the thread "Tidy the imports in the uploader"
    Then the card in the composer says "Done"
    And the card offers the follow-up "Turn on the lint rule that sorts imports"

  @card.done @card.answers
  Scenario: Ticked follow-ups go when I send
    When I open the thread "Tidy the imports in the uploader"
    And I tick follow-up 1 on the card
    Then the follow-up "Turn on the lint rule that sorts imports" is ticked
    When I press Return
    Then the transcript shows my message "Turn on the lint rule that sorts imports"

  @composer.recap @quiet-thread @gpui-pending
  Scenario: A thread that has sat for a while says what it's for
    When I open the thread "Ship the beta to TestFlight"
    Then the recap box says "Gets the first beta out to testers."
    And the recap box says "Waiting on you to accept the agreement"

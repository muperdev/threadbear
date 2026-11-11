Feature: Staying connected to the runner
  As someone whose runner restarts when it updates
  I want the app to reconnect by itself
  So I never have to do anything about it

  Background:
    Given the app is connected to a runner with the demo threads

  @foundation.rpc
  Scenario: The app reconnects when the runner restarts
    When I open the thread "Morning brief"
    And the runner restarts
    Then the app is connected again
    And the transcript still shows the thread "Morning brief"

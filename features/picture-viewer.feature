Feature: Looking at a picture in the transcript
  As someone reading what an agent found or made
  I want a click on a picture to show it, with the rest of its message a step away
  So I can look without leaving the thread

  Background:
    Given the app is connected to a runner with the demo threads

  @transcript.images
  Scenario: A click on a picture shows it, among the rest of its message's pictures
    When I open the thread "Make uploads resume after a crash"
    And I open the line saying "Worked for 21s"
    And I click picture 2 of the row of 2 pictures
    Then the viewer shows picture 2 of 2 "upload-times.png"

  @transcript.images @swift-pending
  Scenario: The arrows go through the message's pictures and Escape closes
    When I open the thread "Make uploads resume after a crash"
    And I open the line saying "Worked for 21s"
    And I click picture 1 of the row of 2 pictures
    Then the viewer shows picture 1 of 2 "progress-after.png"
    When I step the viewer forward
    Then the viewer shows picture 2 of 2 "upload-times.png"
    When I step the viewer forward
    Then the viewer shows picture 1 of 2 "progress-after.png"
    When I press Escape in the viewer
    Then the viewer is closed

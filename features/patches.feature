Feature: What a folder's patches give a thread
  As someone working in a project with its own tools
  I want its chips, panels and the code they run laid out where I can see them
  So I know what's there and can trust it on purpose

  Background:
    Given the app is connected to a runner with the demo threads

  @patches.list
  Scenario: The Patches list says where each patch comes from and what it gives
    When I open the thread "Add dark mode to the dashboard"
    And I show the "threadbear.patches" section of the pane
    Then the patch "tools" comes from "project"
    And the patch "tools" gives "actions Typecheck, Open logs, Deploy to prod, Reseed dev DB"
    And the patch "convex" comes from "built in"

  @patches.list
  Scenario: A patch that isn't trusted waits, and Trust brings it in
    When I open the thread "Pick a colour for the new Share button"
    And I show the "threadbear.patches" section of the pane
    Then the patch "release" is waiting to be trusted, saying "Not run yet"
    When I trust the patch "release"
    Then the patch "release" gives "action Upload build"

  @panel.list
  Scenario: A list panel draws its rows under their headings
    When I open the thread "Rebuild the three landing pages"
    And I show the "deploys.previews" section of the pane
    Then the list panel has the heading "This branch" with the rows "Home | Pricing | Docs"
    And the list panel has the heading "Elsewhere" with the rows "Local | Production"
    And the list row "Pricing" says "12m" and "Waiting on the plan prices"

  @panel.image
  Scenario: An image panel draws its picture
    When I open the thread "Pick a colour for the new Share button"
    And I show the "xcode.simulator" section of the pane
    Then the image panel shows a picture

  @patches.run
  Scenario: A chip runs its action and it lands in the transcript
    When I open the thread "Add dark mode to the dashboard"
    Then the chip "tools.reseed-dev-db" is offered
    When I run the chip "tools.reseed-dev-db"
    Then the transcript shows a line about "Reseed dev DB"

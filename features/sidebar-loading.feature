Feature: The sidebar while it waits for the first list
  As someone opening the app
  I want the sidebar to look like it's loading rather than empty
  So I don't think my threads have gone

  @sidebar.skeleton
  Scenario: Grey rows show until the runner's first list arrives
    Then the sidebar is showing loading rows
    And the sidebar then lists "Morning brief"

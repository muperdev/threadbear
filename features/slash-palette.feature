Feature: The slash command palette
  As someone who knows there's a command for this
  I want to find it by typing a slash
  So I can run it, or fill in its arguments, without leaving the box

  Background:
    Given the app is connected to a runner with the demo threads
    And I open the thread "Morning brief"

  @composer.slash
  Scenario: A slash opens the palette with every command, in categories
    When I type "/" in the composer
    Then the palette is open on "All"
    And the palette's categories say "All 15 | Threadbear 2 | Claude Code 6 | Skills 7"
    And the palette lists "/model" first
    And the palette has the command "/code-review"

  @composer.slash
  Scenario: The picked command is described in full
    When I type "/code" in the composer
    Then the palette's pick is "/code-review"
    And the palette's details say the usage "/code-review [low|medium|high] [target]"
    And the palette's details say they're from "Your skill"

  @composer.slash
  Scenario: Typing narrows the list, names first
    When I type "/co" in the composer
    Then the palette lists "/compact" first
    And the palette has the command "/computercub"

  @composer.slash
  Scenario: A category narrows the list
    When I type "/" in the composer
    And I click the palette's "Skills" category
    Then the palette is open on "Skills"
    And the palette's commands are "/code-review | /computercub | /cloudflare | /claude-api | /deslop | /delegate | /ship"

  @composer.slash
  Scenario: Recent shows once a command has been picked
    When I type "/mod" in the composer
    Then the palette's pick is "/model"
    When I press Tab
    Then the composer says "/model "
    And the palette is closed
    When I type "/" in the composer, replacing what's there
    Then the palette's categories say "All 15 | Threadbear 2 | Claude Code 6 | Skills 7 | Recent 1"

  @composer.slash.keys
  Scenario: The arrows move the pick and stop at the ends
    When I type "/" in the composer
    Then the palette's pick is "/model"
    When I press the down arrow
    And I press the down arrow
    Then the palette's pick is "/clear"
    When I press the up arrow
    Then the palette's pick is "/effort"
    When I press the up arrow
    And I press the up arrow
    Then the palette's pick is "/model"

  @composer.slash.keys
  Scenario: Return runs the picked command
    When I type "/cle" in the composer
    Then the palette's pick is "/clear"
    When I press Return
    Then the palette is closed
    And the transcript shows my message "/clear"
    And the composer is empty

  @composer.slash.keys
  Scenario: Tab fills it in, for its arguments
    When I type "/cod" in the composer
    Then the palette's pick is "/code-review"
    When I press Tab
    Then the composer says "/code-review "
    And the palette is closed

  @composer.slash.keys
  Scenario: Escape closes it until the words change
    When I type "/mod" in the composer
    Then the palette is open on "All"
    When I press Escape
    Then the palette is closed
    And the composer says "/mod"
    When I type "e" in the composer
    Then the palette is open on "All"

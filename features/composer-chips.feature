Feature: The chips under the message box
  As someone choosing how a thread runs
  I want the model, access and folder right where I type
  So I can change them without leaving the box

  Background:
    Given the app is connected to a runner with the demo threads
    And I open the thread "Morning brief"

  @composer.permission
  Scenario: The permission chip says how the thread runs and changes it
    Then the "permission" menu shows "Full access" ticked
    When I pick "Ask before edits" from the "permission" menu
    Then the composer's permission mode is "ask"
    And the "permission" menu shows "Ask before edits" ticked

  @composer.modelpicker
  Scenario: The model picker lists the models, marking the default
    Then the "model" menu has a row "[x] Fable 5.1 *"
    And the "model" menu has a row "[ ] Opus 5.5"
    And the model chip says "Claude · Fable 5.1"

  @composer.modelpicker
  Scenario: Picking a model changes the chip
    When I pick "Opus 5.5" from the "model" menu
    Then the model chip says "Claude · Opus 5.5"

  @composer.modelpicker
  Scenario: Picking an effort level changes the chip
    When I pick "High" from the "model" menu
    Then the model chip says "Claude · Fable 5.1 · High"

  @composer.contextring
  Scenario: The context ring says how full the thread is
    When I open the thread "Make uploads resume after a crash"
    Then the "context" menu says "642k of 1M tokens" and "64%"

  @composer.contextring @gpui-pending
  Scenario: The context ring's popover says what fills a Claude thread's context
    When I open the thread "Make uploads resume after a crash"
    And I open the context ring's popover
    Then the context popover shows "64% used 642k / 1M"
    And the context popover shows "Messages 610k 61%"
    And the context popover shows "MCP tools (deferred) 173k"
    And the context popover shows "Breakdown from just now"

Feature: Decisions on the Projects page
  As someone with several threads waiting on me in one project
  I want their questions in one place, answered one after another
  So I don't open each thread to answer it, and nothing is relayed

  Background:
    Given the app is connected to a runner with the demo threads

  @projects.decisions @project-decisions @gpui-pending
  Scenario: A project's card says how many decisions are waiting
    When I show the Projects page
    Then the project card "Threadbear" says "4 decisions"

  @projects.decisions @project-decisions @gpui-pending
  Scenario: A project's page opens on its threads' questions
    When I open the project "Threadbear"
    Then the project's decisions include "Rename the Settings tabs" asking "Call the Advanced tab \"Machine\" or \"Runner\"?"
    And the project's decisions include "Draft the release notes for 0.4" asking "Lead with projects or with the librarian?"

  @projects.decisions @project-decisions @gpui-pending
  Scenario: Answering on the page answers the thread
    When I open the project "Threadbear"
    And I pick answer 1 of ask 1 on the decision from "Compare SQLite and plain files for the runner"
    Then the project's decisions don't include "Compare SQLite and plain files for the runner"
    When I open the thread "Compare SQLite and plain files for the runner"
    Then the transcript shows my message "Keep plain files for now?"

  @projects.decisions @project-decisions @gpui-pending
  Scenario: A card of several questions waits until each has an answer
    When I open the project "Threadbear"
    And I pick answer 1 of ask 1 on the decision from "Draft the release notes for 0.4"
    Then the project's decisions include "Draft the release notes for 0.4" asking "Mention the Windows build?"
    When I pick answer 2 of ask 2 on the decision from "Draft the release notes for 0.4"
    Then the project's decisions don't include "Draft the release notes for 0.4"

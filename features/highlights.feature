Feature: Highlights, what a thread has made and shared
  As someone keeping an eye on an agent's work
  I want what it made, shared and pinned kept beside the conversation
  So the files, links and commits don't scroll away with it

  Background:
    Given the app is connected to a runner with the demo threads

  @highlights.panel
  Scenario: A thread's highlights are grouped under their headings
    When I open the thread "Rebuild the three landing pages"
    Then Highlights has the headings "Pinned | Files | Links | Changes"

  @highlights.panel
  Scenario: A heading with nothing under it isn't drawn
    When I open the thread "Rebuild the three landing pages"
    Then Highlights has no heading "Needs you"
    And Highlights has no heading "Sub-agents"

  @highlights.panel
  Scenario: A thread with nothing to show says so
    When I open the thread "Set up the Cloudflare tunnel for the preview server"
    Then Highlights says "Nothing here yet. Files, links and commits from this thread show up here as it works."

  @highlights.panel
  Scenario: Folding a heading keeps its count and folds it for every thread
    Given I open the thread "Rebuild the three landing pages"
    When I fold the "links" heading in Highlights
    Then the "Links" heading in Highlights is folded and counts 3
    When I open the thread "Make uploads resume after a crash"
    Then the "Links" heading in Highlights is folded and counts 1

  @highlights.librarian
  Scenario: The librarian's line says where the thread is up to
    When I open the thread "Rebuild the three landing pages"
    Then Highlights opens with the point "The home page is rebuilt and pushed."

  @highlights.pinned
  Scenario: A pin says why, and files and links say how they turned up
    When I open the thread "Rebuild the three landing pages"
    Then the "Pinned" highlight "home.html" has the note "The new hero with the three plan cards under it. Is the headline too long?"
    And the "Pinned" highlight "home.html" sits in "mockups"
    And the "Files" highlight "copy-review.md" says "Written"
    And the "Files" highlight "copy-review.md" sits in "docs"
    And the "Links" highlight "mikecann/site#48" says "Created"

  @highlights.changes
  Scenario: Commits say where they went and the edited files fold away
    When I open the thread "Rebuild the three landing pages"
    Then the "Changes" highlight "Rebuild the home page on the new hero" has the sha "9f3c2e1"
    And the "Changes" highlight "Rebuild the home page on the new hero" sits in "landing-rebuild · pushed"
    And the "Changes" highlight "5 files edited" is listed
    And Highlights doesn't list "Hero.astro"
    When I open the edited files in Highlights
    Then the "Changes" highlight "Hero.astro" sits in "src/components"

  @highlights.needsyou
  Scenario: Ticking an ask in Highlights ticks it on the card, and the count follows
    When I open the thread "Ship the beta to TestFlight"
    Then the "Needs you" highlight "Accept the new agreement in App Store Connect" sits in "App Store Connect › Business"
    And the ask "Accept the new agreement in App Store Connect" is not ticked
    And the "Needs you" heading in Highlights counts 2 when folded
    When I tick the ask "Accept the new agreement in App Store Connect"
    Then the ask "Accept the new agreement in App Store Connect" is ticked
    And the "Needs you" heading in Highlights counts 1 when folded

  @highlights.projects
  Scenario: A project's block lists its links and says why the thread's in it
    When I open the thread "Pick a colour for the new Share button"
    Then the "Walkies" highlight "Live site" sits in "walkies.example.app"
    When I ask why the thread is in the project "Walkies"
    Then it says "Picks the share button colour for Walkies"
    And it says "The librarian put it here."
    And it offers "Keep, Take out, Open project"

  @highlights.subagents
  Scenario: Sub-agents are listed with the ones still running first
    When I open the thread "Why is the build slow on CI?"
    Then the "Sub-agents" highlight "Time the test step on each commit" comes first
    And the "Sub-agents" highlight "Time the test step on each commit" is marked "circle.dashed"
    And the "Sub-agents" highlight "Compare the cache keys" is marked "checkmark"

  @highlights.started
  Scenario: A thread it started is listed with where it's up to
    When I open the thread "Make uploads resume after a crash"
    Then the "Threads it started" highlight "Rename the Settings tabs" is listed

  @highlights.scheduled
  Scenario: What will message the thread later is listed with when
    When I open the thread "Why is the build slow on CI?"
    Then the "Scheduled" highlight "Check the CI run again" is listed
    And the "Scheduled" highlight "Check the CI run again" says "in 40m"

  @highlights.rowmenus
  Scenario: A row's right-click menu offers what can be done with it
    When I open the thread "Rebuild the three landing pages"
    And I right-click the highlight "copy-review.md"
    Then its menu offers "Preview here | Open | Reveal in Finder | Copy path | --- | Remove"
    When I right-click the highlight "9f3c2e1"
    Then its menu offers "Copy sha | --- | Remove"
    When I right-click the highlight "mikecann/site#48"
    Then its menu offers "Copy link | Remove"

  @highlights.rowmenus
  Scenario: Remove takes a row out of Highlights
    When I open the thread "Rebuild the three landing pages"
    And I choose "Remove" from the menu of the highlight "copy-review.md"
    Then Highlights doesn't list "copy-review.md"

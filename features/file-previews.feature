Feature: Files opened in the side pane
  As someone reviewing what an agent made
  I want a file to open beside the conversation, drawn for what it is
  So I can read it without leaving the thread

  Background:
    Given the app is connected to a runner with the demo threads

  @file.host
  Scenario: A click on a file opens it over Highlights, and Back comes back
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    Then the pane is showing the file "docs/copy-review.md"
    When I press Back in the pane
    Then the pane shows Highlights

  @file.markdown
  Scenario: A Markdown file is drawn as a page
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "copy-review.md"
    Then the pane shows a "markdown" page with the words "Copy review: home, pricing, docs"
    And the pane's page has the words "Nine words, down from twenty."

  @file.textview
  Scenario: A code file is shown as selectable, coloured text
    When I open the thread "Rebuild the three landing pages"
    And I open "src/hero.ts" in the thread's folder in the pane
    Then the pane shows a "code" page with the words "export function hero"

  @file.textview
  Scenario: A plain text file is shown as it is
    When I open the thread "Rebuild the three landing pages"
    And I open "logs/docs-build.txt" in the thread's folder in the pane
    Then the pane shows a "text" page with the words "Could not resolve image"

  @file.picture
  Scenario: A picture is drawn in the pane, and Copy Image copies the picture itself
    When I open the thread "Rebuild the three landing pages"
    And I click the highlight "pricing-before.png"
    Then the pane shows a picture
    When I copy the picture in the pane
    Then it was copied as a picture

# Where each thread action works

Made from `runner/src/parity.ts` by `npm run parity-docs` in `runner/`; edit that, not this.
`runner/test/unit/parity.test.ts` checks each row against the code, and fails when a new
thread request has no row, so a feature can't go missing on another Mac or the phone
without someone deciding it should.

"Another Mac" is a thread on another runner opened in the Mac app through Convex, the
default. With Settings > Runners > Connect to other machines directly on, the app talks
to that runner itself and everything works. "Phone and web" is the iPhone app and the
website, which read every thread through Convex.

| What | Request | Another Mac | Phone and web |
|---|---|---|---|
| Reading the transcript | `threads.events` | Yes | Yes |
| Following it live | `threads.subscribe` | Yes | Yes |
| Opening a tool's output | `threads.heldBody` | Yes | Yes |
| Sending a message, pictures too | `threads.send` | Yes | Yes |
| Stop | `threads.interrupt` | Yes | Yes |
| Rename, archive, star, hibernate | `threads.update` | Yes | Yes |
| Marking it read | `threads.markSeen` | Yes | Yes |
| Sharing by link | `threads.share` | Yes | Yes |
| Answering an approval or a question | `approvals.respond` | Yes | Yes |
| Answering a secret card | `secrets.answer` | Yes | Yes |
| Pressing a reply button | `buttons.press` | Yes | Yes |
| A message's reply buttons | `buttons.list` | Yes | Yes |
| The slash command list | `threads.commands` | Yes | Yes |
| Hibernate until something happens | `threads.hibernateWhen` | Yes | No: The phone hibernates until a time only. |
| What fills its context | `threads.contextUsage` | No: Asked of the CLI on that Mac; the ring's popover there shows the totals only. | No: Not on the phone. |
| Ticking an ask on the card | `threads.tickAsk` | No: Ticks stay on that Mac. | No: The phone sends picks with its answer instead. |
| Editing a message | `threads.edit` | No: It undoes file changes on that Mac, so it waits for a way to show what it undid. | No: Not on the phone. |
| Deleting it | `threads.delete` | No: Done on that Mac. | No: Not on the phone. |
| Its highlights | `threads.highlights` | No: Highlights are files on that Mac. | No: Not on the phone. |
| Removing a highlight | `threads.removeHighlight` | No: Highlights are files on that Mac. | No: Not on the phone. |
| Explain | `threads.explain` | No: Runs a model on that Mac. | No: Not on the phone. |
| Reading a sub-agent's work | `threads.subagent` | No: Sub-agents stay on that Mac. | No: Not on the phone. |
| A panel being shown | `threads.panelShown` | No: Patches run on that Mac. | No: No panels on the phone. |
| Opening a panel | `threads.showPanel` | No: Patches run on that Mac. | No: No panels on the phone. |
| Drawing a panel | `panels.render` | No: Patches run on that Mac. | No: No panels on the phone. |
| The Convex panel's key | `panels.convexKey` | No: Patches run on that Mac. | No: No panels on the phone. |
| The Convex panel's deployments | `panels.convexDeployments` | No: Patches run on that Mac. | No: No panels on the phone. |
| The Convex panel's login | `panels.convexLogin` | No: Patches run on that Mac. | No: No panels on the phone. |
| An action chip | `actions.run` | No: Patches run on that Mac. | No: No action chips on the phone. |
| Trusting a folder's patches | `patches.trust` | No: Trust is given on that Mac. | No: Not on the phone. |
| Approving a plan | `plans.approve` | No: Not sent through Convex from the Mac yet. | No: Not on the phone yet. |
| Design reviews | `reviews.list` | No: Reviews open in the Mac app from that Mac. | No: Not on the phone. |
| Starting a design review | `reviews.create` | No: Reviews open in the Mac app from that Mac. | No: Not on the phone. |
| Reading a design review | `reviews.get` | No: Reviews open in the Mac app from that Mac. | No: Not on the phone. |
| Saving a design review | `reviews.save` | No: Reviews open in the Mac app from that Mac. | No: Not on the phone. |
| Answering a design review | `reviews.respond` | No: Reviews open in the Mac app from that Mac. | No: Not on the phone. |
| Cancelling a design review | `reviews.cancel` | No: Reviews open in the Mac app from that Mac. | No: Not on the phone. |

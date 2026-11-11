# Behaviour specs

Gherkin `.feature` files for what Threadbear's desktop apps do, run with
[Cucumber.js](https://github.com/cucumber/cucumber-js) against both the Swift
Mac app and the GPUI app (`desktop/`). The Swift app is the definition of right
until it's retired: a scenario is written against it first, then the GPUI app
has to pass the same file. See `docs/gpui-port.md`.

## What a scenario is for

As in Let's Build It, `.feature` files are an **intent contract for designed
behaviour**, not a regression dragnet. A scenario earns its place when it
describes something a person using the app experiences.

Use them for user-visible flows, designed empty and error states, and state
changes you can watch happen. Don't use them for exhaustive option matrices,
implementation details, or how something looks: pixels and styling are for
screenshots and component tests.

**Rendering rule.** A scenario about content asserts on what's drawn (the text
of the reply, the card's command), not that a container appeared.

**Behaviour first.** For anything a user can see, write the scenario before the
code, and make it fail first when you can.

## How it's laid out

- `<name>.feature` is owned by `steps/<name>.steps.ts`. Shared steps go in
  `steps/_common.steps.ts`; files starting with `_` don't own a feature.
- Every scenario carries a tag naming the row it covers in
  `docs/gpui-port/features.md`, such as `@sidebar.search`. `verify-pairing.mjs`
  checks both of these, and `parity.mjs` builds the table in
  `docs/gpui-port/PARITY.md` from them.
- `@gpui-pending` or `@swift-pending` means that app doesn't pass it yet.
  `@manual` means nobody's found a way to automate it (a real trackpad gesture,
  say). The table counts both as not done.
- `@long-thread` runs a scenario on a made-up long thread and one short demo
  thread (`support/long-thread.ts`), for what only shows on more than a page
  of events. `@remote-threads` turns on the mock's other-Mac threads
  (`bruce_*`), which it leaves out otherwise.
- `support/` holds the drivers. Both apps take the same JSON commands and give
  the same `dump` (`app/README.md`, "Seeing and driving it from a script"), so
  a step never asks which app it's driving.

## Running them

Each scenario starts its own app on its own mock runner, about 6 seconds apiece.

```bash
npm ci                                  # once, in this folder
npm run check                           # steps line up, no apps needed

# Build the app first, then:
TB_APP=gpui  node_modules/.bin/cucumber-js        # or swift
TB_TAGS='@sidebar.search' ...                     # just these
TB_INCLUDE_PENDING=1 ...                          # to see what's left
```

Never `npx cucumber-js`: if the package isn't installed it goes and fetches
one with that name, and it isn't this one.

The GPUI app needs `cargo build -p threadbear-gpui --features automation`, and
the Swift app needs `swift build --product Threadbear` in `app/`. Whole runs
go on another machine (AGENTS.md):

```bash
desktop/scripts/remote.sh cloud -- bash -c 'cd features && TB_APP=gpui node_modules/.bin/cucumber-js'
desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd features && TB_APP=swift node_modules/.bin/cucumber-js'
```

A failed scenario leaves a screenshot in `reports/failures/`.

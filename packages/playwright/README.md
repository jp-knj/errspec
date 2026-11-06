# Playwright mutation fixture

The experimental browser adapter tests app code served by Vite. Use Node 24+, a local `@playwright/test` 1.64+ installation and Vite, and install the browsers selected by your Playwright projects.

Import the automatic fixture in mutation tests:

```js
import {test, expect} from "@errspec/playwright";
test("counter", async ({page}) => {
  await page.goto("/");
  await expect(page.getByRole("button")).toHaveText("1");
});
```

Existing fixtures can extend `mutationFixtures`:

```js
import {mutationFixtures} from "@errspec/playwright";
export const test = applicationTest.extend(mutationFixtures);
```

Run `errspec mutate --runner playwright --runner-config playwright.config.ts`. All configured projects run unless `--runner-project NAME` selects one. Keep page URLs relative to `baseURL`: errspec starts the app's Vite configuration on a separate local port for each owned worker. The adapter provides its own instrumentation plugin. An existing `vite`/`vp dev` webServer command is replaced by that server; custom webServer commands are rejected.

Each request starts fresh browser workers. Test IDs include the project, file, name and duplicate occurrence. The fixture records hits from documents and frames through a browser binding, retaining observations after navigation or page closure. Failed fixture acquisition never supplies a detection result. Missing fixture metadata makes the baseline fail with an actionable diagnostic. Outside errspec runs the fixture only calls the original test.

This adapter covers page and frame code served directly by Vite. Jest CommonJS, Vitest Browser Mode, custom non-Vite app servers, source code bundled outside the Vite transform pipeline and browser worker code are outside this contract. Result and coverage caches conservatively revalidate because the adapter has not acquired every external browser input.

Checked-in fixtures pin Playwright 1.64.1 and Vite 8.3.4. `tests/mutation-runners.test.mjs` exercises Chromium, multiple projects, extended fixtures, navigation, frames and independent parallel workers. No package is published by these development commands.

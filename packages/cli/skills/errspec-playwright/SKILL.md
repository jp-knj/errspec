---
name: errspec-playwright
description: Set up or diagnose errspec mutation testing of Vite browser code exercised by Playwright Test, including the errspec fixture, project selection and per-worker server behavior.
license: MIT
---

# Browser mutation testing

Use Node 24+, the project's Vite installation, Playwright Test 1.64+, its browser
binaries, and matching errspec native/CLI plus `@errspec/playwright` artifacts.
Packages are not published; use the provided tarballs or existing installation.
Keep the project's ordinary Playwright baseline passing before mutation testing.

Import the fixture in every participating test:

```ts
import { test, expect } from '@errspec/playwright';
test('shows the result', async ({page}) => {
  await page.goto('/');
  await expect(page.getByText('Ready')).toBeVisible();
});
```

Projects with existing fixtures can extend their base:

```ts
import {test as base, expect} from '@playwright/test';
import {mutationFixtures} from '@errspec/playwright';
export const test = base.extend(mutationFixtures).extend({ /* existing fixtures */ });
export {expect};
```

```sh
errspec mutate --runner playwright --include 'src/**/*.ts'
# Optional: --runner-config playwright.config.ts --runner-project chromium
```

The adapter injects its Vite mutation plugin and starts a private server/port for
each worker. No user Vite plugin or `webServer` setting is required. Existing
`webServer` commands must be Vite or `vp dev`; custom commands are rejected. Use
relative navigation such as `page.goto('/')`, so the assigned baseURL reaches the
instrumented server rather than an unrelated fixed URL.

`--runner-package` can select the installed `@playwright/test` directory.
Omitting `--runner-project` runs all configured projects; preserve project-specific
opaque IDs in reports and selected/matrix requests. Fixture extensions remain
typed and can be chained with the application's own fixtures.

If worker evidence is missing, verify imports/extensions and that the browser
actually loads the included Vite sources. Pages, frames and same-origin navigation
are observed; browser WebWorkers are outside the supported coverage scope.
Node-only API tests are not browser mutation evidence. Do not turn missing
instrumentation, unsupported server setup, Pending work or an incomplete run into
a successful verification claim. Use the `errspec-test-loop` skill for actual gaps.

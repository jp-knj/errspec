import { test } from "vitest";
import { scenarioTest } from "../../../packages/testkit/index.mjs";
import { checkout } from "../src/checkout";

scenarioTest(test, import.meta.url, {
  contract: "checkout.payment", id: "declined", tag: "Declined",
}, async (s) => {
  let orders = 0;
  const charge = s.fault("gateway", async () => ({
    ok: false as const, error: { _tag: "Declined" as const, reason: "insufficient funds" },
  }));
  const response = await checkout({ charge, createOrder: async () => { orders++; } });
  s.check("status", response.status, 402);
  s.check("orders", orders, 0);
});

for (const [id, maxAttempts] of [["rate-limit-first", 1], ["rate-limit-retries", 3]] as const) {
  scenarioTest(test, import.meta.url, {
    contract: "checkout.payment", id, tag: "RateLimited",
  }, async (s) => {
    let orders = 0;
    let attempts = 0;
    const charge = s.fault("gateway", async () => {
      attempts++;
      return { ok: false as const, error: { _tag: "RateLimited" as const, retryAfter: 30 } };
    });
    const response = await checkout({ charge, createOrder: async () => { orders++; } }, maxAttempts);
    s.check("status", response.status, 429);
    s.check("orders", orders, 0);
    s.check("attempts", attempts, maxAttempts);
  });
}

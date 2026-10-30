import { test, expect } from "vitest";
import { checkFailureAtomicity } from "../../../packages/testkit/index.mjs";
import { checkout } from "../src/checkout";

test("payment failures leave orders unchanged", async () => {
  const report = await checkFailureAtomicity({
    faults: [
      { id: "Declined", dependency: "charge", inject: () => ({ ok: false, error: { _tag: "Declined", reason: "funds" } }) },
      { id: "RateLimited", dependency: "charge", inject: () => ({ ok: false, error: { _tag: "RateLimited", retryAfter: 30 } }) },
    ],
    isFailure: response => response.status >= 400,
    setup() {
      const orders: string[] = [];
      return {
        snapshot: () => orders,
        run: scope => checkout({
          charge: scope.wrap("charge", async () => ({ ok: true as const })),
          createOrder: async () => { orders.push("order"); },
        }),
      };
    },
  });
  expect(report.cases.map(trial => trial.status)).toEqual(["preserved", "preserved"]);
});

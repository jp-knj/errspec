import { mock, test } from "bun:test";
import { scenarioTest } from "../../../packages/testkit/index.mjs";
import type { Delivery } from "../src/mailer";
import { notify } from "../src/notify";

// mock.module returns a forwarder; each scenario assigns its own fault to `mailer.deliver`.
const mailer = { deliver: async (_to: string): Promise<Delivery> => ({ accepted: true }) };
mock.module("../src/mailer", () => ({ deliver: (to: string) => mailer.deliver(to) }));

const scenarios = [
  { id: "sent", tag: "sent", deliver: async () => ({ accepted: true }) },
  { id: "rejected", tag: "rejected", deliver: async () => ({ accepted: false }) },
  { id: "offline", tag: "retry_later", deliver: async (): Promise<Delivery> => { throw new Error("offline"); } },
] as const;

for (const { id, tag, deliver } of scenarios) {
  scenarioTest(test, import.meta.url, { contract: "notify", id, tag }, async (s) => {
    mailer.deliver = s.fault("mailer", deliver);
    s.check("result", await notify("user@example.com"), { status: tag });
  });
}

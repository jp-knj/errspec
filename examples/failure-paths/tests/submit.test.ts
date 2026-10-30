import { test } from "bun:test";
import { scenarioTest } from "../../../packages/testkit/index.mjs";
import { submit } from "../src/submit";

const send = async () => {};

scenarioTest(test, import.meta.url, {
  contract: "signup", id: "invalid-email", tag: "invalid", withoutFaults: true,
}, async (s) => {
  s.check("result", await submit("nobody", { verifyHuman: async () => true, send }), { status: "invalid" });
});

scenarioTest(test, import.meta.url, {
  contract: "signup", id: "verification-down", tag: "blocked",
}, async (s) => {
  const verifyHuman = s.fault("verification", async (): Promise<boolean> => { throw new Error("down"); });
  s.check("result", await submit("a@example.com", { verifyHuman, send }), { status: "blocked" });
});

scenarioTest(test, import.meta.url, {
  contract: "signup", id: "bot", tag: "blocked",
}, async (s) => {
  const verifyHuman = s.fault("verification", async () => false);
  s.check("result", await submit("a@example.com", { verifyHuman, send }), { status: "blocked" });
});

scenarioTest(test, import.meta.url, {
  contract: "signup", id: "sent", tag: "sent",
}, async (s) => {
  const verifyHuman = s.fault("verification", async () => true);
  s.check("result", await submit("a@example.com", { verifyHuman, send }), { status: "sent" });
});

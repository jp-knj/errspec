import {test,expect} from "@errspec/playwright";
test("checked",async({page})=>{
  await page.goto("/");
  await page.waitForFunction(()=>typeof globalThis.checked==="function");
  expect(await page.evaluate(()=>globalThis.checked(2))).toBe(3);
});
test.skip("skipped",async()=>{});

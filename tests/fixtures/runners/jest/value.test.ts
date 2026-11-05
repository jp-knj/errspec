import {test,expect} from "@jest/globals";
import {checked} from "./src/value.ts";
test("checked",()=>expect(checked(2)).toBe(3));
test.skip("skipped",()=>expect(checked(2)).toBe(4));

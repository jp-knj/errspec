import {test,expect} from "vite-plus/test";
import {checked} from "./src/value";
test("checked",()=>expect(checked()).toBe(true));

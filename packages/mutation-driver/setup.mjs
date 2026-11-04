// __VITEST_ENTRY__ is replaced with the selected installation's absolute ESM entry.
import { inject, TestRunner, afterAll } from "__VITEST_ENTRY__";
import { attachWeakBinary } from "./weak-runtime.mjs";
import { observeChildProcesses } from "./child-process.mjs";
const context = inject("__errspecMutation");
if (!context) throw new Error("Missing errspec mutation context");
const file = TestRunner.getCurrentSuite().file;
const data = file.meta.__errspecMutation = {
  runId: context.runId, requestId: context.requestId, hits: {}, staticHits: {},
};
const stopObserving=observeChildProcesses(fact=>{
  const task=TestRunner.getCurrentTest();
  (data.childProcesses ||= []).push({...fact,taskId:task&&!concurrent(file.tasks)?task.id:null});
});
afterAll(stopObserving);
function concurrent(tasks) {
  return tasks?.some(t => t.concurrent || t.tasks && concurrent(t.tasks));
}
const symbol = Symbol.for(`errspec.mutation.${context.runId}`);
globalThis[symbol] = key => {
  const count = data.hits[key] = (data.hits[key] || 0) + 1;
  const task = TestRunner.getCurrentTest();
  if (task && !concurrent(file.tasks)) {
    const hits = task.meta.__errspecMutation ||= { runId: context.runId, requestId: context.requestId, hits: {} };
    hits.hits[key] = (hits.hits[key] || 0) + 1;
  } else data.staticHits[key] = true;
  if (key === context.activeKey && count > context.hitLimit) {
    const error = new Error("ERRSPEC_HIT_LIMIT"); error.name = "ErrspecHitLimitError"; throw error;
  }
  return key === context.activeKey;
};
if (context.weakMutation) {
  data.weakVersion = 1;
  attachWeakBinary(globalThis[symbol], key => {
    const task = TestRunner.getCurrentTest();
    if(task&&!concurrent(file.tasks)) {
      const meta = task.meta.__errspecMutation;
      (meta.infected ||= {})[key] = true;
    }
  }, context.activeKey);
}

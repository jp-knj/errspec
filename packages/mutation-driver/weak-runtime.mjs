// Pure runtime operations shared by Node and browser adapters. No eval or duplicated operands.
const operations = {
  "+": (a, b) => a + b,
  "-": (a, b) => a - b,
  "*": (a, b) => a * b,
  "/": (a, b) => a / b,
  "%": (a, b) => a % b,
  "<": (a, b) => a < b,
  "<=": (a, b) => a <= b,
  ">": (a, b) => a > b,
  ">=": (a, b) => a >= b,
  "==": (a, b) => a == b,
  "!=": (a, b) => a != b,
  "===": (a, b) => a === b,
  "!==": (a, b) => a !== b,
};
const primitive = (value) =>
  value === null || (typeof value !== "object" && typeof value !== "function");

export function attachWeakBinary(record, infect, activeKey) {
  record.binary = (originalOperator, mutations, left, right) => {
    let operator = originalOperator;
    for (const [key, replacement] of mutations)
      if (record(key)) operator = replacement;
    if (!operations[operator])
      throw new Error(`Unsupported weak operator ${operator}`);
    const original = operations[operator](left, right);
    if (activeKey == null) {
      for (const [key, replacement] of mutations) {
        if (!primitive(left) || !primitive(right)) {
          infect(key);
          continue;
        }
        try {
          if (!Object.is(original, operations[replacement](left, right)))
            infect(key);
        } catch {
          infect(key);
        }
      }
    }
    return original;
  };
  return record;
}

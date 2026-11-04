// Acquire entry identities and return locations with the selected compiler.
// Filtering by contract tags and escaping-effect judgments remain in Rust.
import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { sorted, TagError } from "./compiler.mjs";

export function entryFacts(context, syn, root, request) {
  const { entry, discriminant } = request;
  const file = context.program.getSourceFile(realpathSync(resolve(root, entry.file)));
  if (!file) throw new TagError(`entry ${entry.file} is not in the TypeScript project`);
  const module = context.symbol(file);
  if (!module) throw new TagError("entry file must be a module");
  const symbol = context.unalias(context.checker.getExportsOfModule(module).find(s => s.name === entry.export));
  if (!symbol) throw new TagError(`entry export ${entry.export} not found in ${entry.file}`);
  const { K, ModifierFlags } = syn;
  const fn = context.declarations(symbol).map(d => d.kind === K.FunctionDeclaration && d.body ? d
    : d.kind === K.VariableDeclaration && [K.ArrowFunction, K.FunctionExpression].includes(d.initializer?.kind) ? d.initializer : undefined).find(Boolean);
  if (!fn) throw new TagError(`entry ${entry.export} must be a function declaration or a const function`);
  const returns = [];
  if (fn.body.kind === K.Block) {
    syn.walkBody(fn.body, n => {
      if (n.kind === K.ReturnStatement && n.expression) returns.push([n, n.expression]);
    });
  } else returns.push([fn.body, fn.body]);
  const { TypeFlags: T } = context.constants;
  const isAsync = syn.modifier(fn, ModifierFlags.Async);
  const paths = [];
  for (const [statement, expression] of returns) {
    const type = context.type(expression);
    const members = isAsync ? context.awaitedMembers(type) : context.members(type);
    const values = [];
    for (const member of members) {
      if (member.flags & (T.Any | T.Unknown | T.Never)) continue;
      const property = context.propertyType(member, discriminant);
      if (property) for (const value of context.members(property)) {
        if (value.flags & T.StringLiteral) values.push(value.value);
      }
    }
    const tags = sorted(values);
    if (tags.length) paths.push({ ...syn.location(statement), tags });
  }
  return { function: syn.key(fn), paths };
}

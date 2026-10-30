import { sorted } from "./compiler.mjs";

export function scenarios(context, syn, files) {
  const { K, walk, text, location } = syn,
    {
      checker,
      constants: { TypeFlags: T },
    } = context;
  const sdk = (call, name, owner) => {
    const declaration = context.declaration(context.signature(call));
    if (!declaration?.name || text(declaration.name) !== name) return false;
    if (owner)
      return (
        declaration.parent?.kind === K.InterfaceDeclaration &&
        declaration.parent.name &&
        text(declaration.parent.name) === owner
      );
    const module = context.symbol(declaration.getSourceFile());
    return (
      !!module &&
      checker.getExportsOfModule(module).some((s) => s.name === "ScenarioScope")
    );
  };
  const literals = (type) => {
    const values = [];
    for (const member of context.members(type)) {
      if (member.flags & T.StringLiteral) values.push(member.value);
      else if (member.flags & T.BooleanLiteral)
        values.push(context.typeString(member));
      else return;
    }
    return values.length ? sorted(values) : undefined;
  };
  const value = (object, name) => {
    for (const property of object.properties) {
      if (!property.name || text(property.name) !== name) continue;
      const expression =
        property.kind === K.PropertyAssignment
          ? property.initializer
          : property.kind === K.ShorthandPropertyAssignment
            ? property.name
            : undefined;
      if (!expression) return { kind: "unknown" };
      const values = literals(context.type(expression));
      return values ? { kind: "known", values } : { kind: "unknown" };
    }
    return {
      kind: object.properties.some((p) => p.kind === K.SpreadAssignment)
        ? "unknown"
        : "absent",
    };
  };
  const nested = (call) => {
    for (let parent = call.parent; parent; parent = parent.parent) {
      if (parent.kind !== K.CallExpression) continue;
      let root = parent.expression;
      while ([K.PropertyAccessExpression, K.CallExpression].includes(root.kind))
        root = root.expression;
      if (
        root.kind === K.Identifier &&
        context.symbol(root)?.name === "describe"
      )
        return true;
    }
    return false;
  };
  const result = [];
  for (const name of files) {
    const file = context.program.getSourceFile(name);
    if (!file) continue;
    walk(file, (call) => {
      if (call.kind !== K.CallExpression || !sdk(call, "scenarioTest")) return;
      const definition = call.arguments?.[2],
        body = call.arguments?.[3],
        uses = [];
      if (body)
        walk(body, (n) => {
          if (n.kind !== K.CallExpression) return;
          for (const kind of ["fault", "check"])
            if (sdk(n, kind, "ScenarioScope") && n.arguments?.[0]) {
              const values = literals(context.type(n.arguments[0]));
              if (values) for (const value of values) uses.push([kind, value]);
            }
        });
      const definitionObject = definition?.kind === K.ObjectLiteralExpression;
      result.push({
        location: location(call),
        insideDescribe: nested(call),
        contract: definitionObject
          ? value(definition, "contract")
          : { kind: "unknown" },
        id: definitionObject ? value(definition, "id") : { kind: "unknown" },
        tags: definitionObject ? value(definition, "tag") : { kind: "unknown" },
        withoutFaults: definitionObject
          ? value(definition, "withoutFaults")
          : { kind: "unknown" },
        uses,
      });
    });
  }
  return result;
}

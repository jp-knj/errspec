// Collect compiler-independent facts; propagation remains in Rust.
import { sorted, order, TagError } from "./compiler.mjs";

export function scan(context, syn, root, discriminants, extractTags) {
  const {
    K,
    NodeFlags,
    ModifierFlags,
    location,
    text,
    key,
    functionLike,
    walk,
    walkBody,
    modifier,
    local,
  } = syn;
  const {
    checker,
    constants: { TypeFlags: T, SymbolFlags: S },
  } = context;
  const facts = {
    files: [],
    candidates: [],
    functions: [],
    origins: [],
    edges: [],
    unknowns: [],
  };
  const files = context.files.filter(local),
    bodies = new Map(), declared = new Map(), errorDeclarations = [];
  const declarationKey = node => {
    const loc = location(node);
    return `${loc.file}:${loc.byteOffset}`;
  };
  const declarationsOf = type => {
    const result = new Set();
    const visit = type => {
      const symbols = context.major === 6 ? [type.aliasSymbol, type.symbol]
        : [type.getAliasSymbol(), type.getSymbol()];
      for (const symbol of symbols) {
        const declaration = context.declarations(symbol)[0];
        if (declaration) result.add(declarationKey(declaration));
      }
      if (type.flags & T.Union) for (const member of context.members(type)) visit(member);
    };
    visit(type);
    return result;
  };
  const external = (type, discriminant) => context.members(type).every(member => {
    const property = checker.getPropertyOfType(member, discriminant);
    const declaration = context.declarations(property)[0];
    return !!declaration && !local(declaration.getSourceFile());
  });
  const isConst = (decl) =>
    decl.parent?.kind === K.VariableDeclarationList &&
    !!(decl.parent.flags & NodeFlags.Const);
  const binding = (node, seen = new Set()) => {
    if (!node || seen.has(node)) return;
    seen.add(node);
    if ([K.ArrowFunction, K.FunctionExpression].includes(node.kind))
      return node;
    if (
      [
        K.ParenthesizedExpression,
        K.AsExpression,
        K.SatisfiesExpression,
        K.TypeAssertionExpression,
        K.NonNullExpression,
      ].includes(node.kind)
    )
      return binding(node.expression, seen);
    if (node.kind !== K.Identifier) return;
    const symbol = context.unalias(context.symbol(node));
    const declarations = context.declarations(symbol);
    const fn = declarations.find(
      (d) => d.kind === K.FunctionDeclaration && d.body,
    );
    if (fn) return fn;
    const decl = declarations.find((d) => d.kind === K.VariableDeclaration);
    if (decl && context.builtin(decl)) return decl;
    if (decl && isConst(decl)) return binding(decl.initializer, seen);
  };
  const caught = (node) => {
    let current = node;
    while (current.parent) {
      const parent = current.parent;
      if (functionLike(parent)) break;
      if (
        parent.kind === K.TryStatement &&
        parent.tryBlock === current &&
        parent.catchClause
      ) {
        let rethrows = false;
        walkBody(parent.catchClause.block, (n) => {
          if (n.kind === K.ThrowStatement) rethrows = true;
        });
        return [true, rethrows];
      }
      current = parent;
    }
    return [false, false];
  };
  const direct = (node) => {
    const [catches, mayRethrow] = caught(node),
      usage = { awaited: false, returned: false, catches, mayRethrow };
    let current = node;
    while (current.parent) {
      const parent = current.parent;
      if (parent.kind === K.AwaitExpression) usage.awaited = true;
      else if (
        parent.kind === K.ReturnStatement ||
        (parent.kind === K.ArrowFunction && parent.body === current)
      ) {
        usage.returned = true;
        break;
      } else if (
        ![
          K.ParenthesizedExpression,
          K.AsExpression,
          K.SatisfiesExpression,
          K.TypeAssertionExpression,
          K.NonNullExpression,
        ].includes(parent.kind)
      )
        break;
      current = parent;
    }
    return usage;
  };
  const usages = (expression, body) => {
    const result = [direct(expression)],
      queue = [expression],
      seen = new Set();
    while (queue.length) {
      const value = queue.pop(),
        decl = value.parent;
      if (
        !decl ||
        decl.kind !== K.VariableDeclaration ||
        !isConst(decl) ||
        decl.initializer !== value ||
        seen.has(decl)
      )
        continue;
      seen.add(decl);
      const symbol = context.symbol(decl.name);
      if (!symbol) continue;
      walkBody(body, (n) => {
        if (
          n.kind === K.Identifier &&
          n !== decl.name &&
          context.symbol(n) === symbol
        ) {
          result.push(direct(n));
          queue.push(n);
        }
      });
    }
    return result;
  };
  const unknown = (owner, node, reason) =>
    facts.unknowns.push({
      function: owner,
      location: location(node),
      expression: (() => {
        const original = text(node), line = original.split(/\r?\n/, 1)[0].trimEnd();
        const chars = [...line];
        return original === line && chars.length <= 120 ? original : chars.slice(0, 120).join("") + "…";
      })(),
      reason,
    });
  const origin = (owner, node, channel, errorType, caught, escapes) =>
    facts.origins.push({
      id: `${key(node)}:${{ throw: "Throw", rejection: "Rejection", result: "Result" }[channel]}`,
      function: owner,
      location: location(node),
      channel,
      errorType,
      caught,
      escapes,
    });
  const resultErrors = (type) => {
    const errors = [];
    function collect(t) {
      for (const member of context.members(t)) {
        if (member.flags & (T.Any | T.Unknown | T.Never)) continue;
        const error = context.propertyType(member, "error");
        if (error) errors.push(error);
      }
    }
    collect(type);
    return errors;
  };
  for (const file of files) {
    facts.files.push(location(file).file);
    const module = context.symbol(file);
    if (module)
      for (const exported of checker.getExportsOfModule(module)) {
        const symbol = context.unalias(exported);
        if (!(symbol.flags & (S.TypeAlias | S.Interface | S.Class))) continue;
        for (const discriminant of discriminants) {
          try {
            const values = extractTags({
              file: file.fileName,
              export: exported.name,
              discriminant,
            });
            if (values.length) {
              const declaration = context.declarations(symbol)[0];
              const identity = declaration ? declarationKey(declaration) : `${location(file).file}:0`;
              const candidateKey = `${identity}:${discriminant}`;
              const candidate = {
                source: {
                  file: location(file).file,
                  export: exported.name,
                  discriminant,
                },
                tags: values,
                reexports: [], external: external(checker.getDeclaredTypeOfSymbol(symbol), discriminant), flows: [],
              };
              if (!declared.has(candidateKey)) declared.set(candidateKey, { identity, file: declaration ? location(declaration).file : location(file).file, exports: [] });
              declared.get(candidateKey).exports.push(candidate);
            }
          } catch (error) {
            if (!(error instanceof TagError)) throw error;
          }
        }
      }
    if (file.isDeclarationFile) continue;
    walk(file, (node) => {
      if (node.kind === K.SourceFile || (functionLike(node) && node.body)) {
        const id = key(node),
          name =
            node.kind === K.SourceFile
              ? "<module>"
              : node.name || node.parent?.name;
        facts.functions.push({
          id,
          name:
            typeof name === "string" ? name : name ? text(name) : "<anonymous>",
          location: location(node),
          isAsync: modifier(node, ModifierFlags.Async),
        });
        bodies.set(id, node);
      }
    });
  }
  const candidateDeclarations = new Map();
  for (const [candidateKey, group] of [...declared].sort(([a],[b]) => order(a,b))) {
    group.exports.sort((a,b) => order(a.source.file,b.source.file) || order(a.source.export,b.source.export));
    let primary = group.exports.findIndex(c => c.source.file === group.file);
    if (primary < 0) primary = 0;
    const [candidate] = group.exports.splice(primary,1);
    candidate.reexports = group.exports.map(c => ({ file: c.source.file, export: c.source.export }));
    candidateDeclarations.set(candidate, group.identity);
    facts.candidates.push(candidate);
  }
  for (const [owner, node] of [...bodies].sort(([a], [b]) => order(a, b))) {
    const isAsync = modifier(node, ModifierFlags.Async),
      body = node.body || node;
    if (node.kind !== K.SourceFile) {
      if (node.asteriskToken)
        unknown(owner, node, "generator_execution_requires_iteration");
      const signature = checker.getSignatureFromDeclaration(node);
      if (!signature)
        throw new Error("Compiler could not resolve a function signature");
      const returned = checker.getReturnTypeOfSignature(signature);
      const errors = context.awaitedMembers(returned).flatMap(resultErrors);
      if (errors.length) {
        errorDeclarations.push([new Set(errors.flatMap(t => [...declarationsOf(t)])), "result"]);
        origin(owner, node, "result", sorted(errors.map(context.typeString)).join(" | "), false, true);
      }
    }
    walkBody(body, (n) => {
      if (n.kind === K.ThrowStatement) {
        const [isCaught, rethrows] = caught(n);
        errorDeclarations.push([declarationsOf(context.type(n.expression)), isAsync ? "rejection" : "throw"]);
        origin(
          owner,
          n,
          isAsync ? "rejection" : "throw",
          context.typeString(context.type(n.expression)),
          isCaught,
          !isCaught || rethrows,
        );
        return;
      }
      if (![K.CallExpression, K.NewExpression].includes(n.kind)) return;
      const signature = context.signature(n),
        declaration = context.declaration(signature),
        uses = usages(n, body),
        callee = n.expression;
      const resolved = binding(callee);
      if (
        n.kind === K.CallExpression &&
        callee.kind === K.PropertyAccessExpression &&
        declaration &&
        declaration.name &&
        text(declaration.name) === "reject" &&
        declaration.parent?.name &&
        text(declaration.parent.name) === "PromiseConstructor" &&
        context.builtin(declaration) &&
        context.builtin(binding(callee.expression) || callee.expression)
      ) {
        const receiver = binding(callee.expression);
        if (receiver?.name && text(receiver.name) === "Promise") {
          if (n.arguments?.[0]) errorDeclarations.push([declarationsOf(context.type(n.arguments[0])), "rejection"]);
          const error = n.arguments?.[0]
            ? context.typeString(context.type(n.arguments[0]))
            : "undefined";
          const escapes = uses.some(
            (u) =>
              (u.returned && !u.awaited) ||
              (u.awaited && (!u.catches || u.mayRethrow)),
          );
          origin(
            owner,
            n,
            "rejection",
            error,
            uses.some((u) => u.awaited && u.catches),
            escapes,
          );
          if (!uses.some((u) => u.returned || u.awaited))
            unknown(owner, n, "unobserved_promise_rejection");
          return;
        }
      }
      if (
        n.kind === K.NewExpression &&
        resolved &&
        context.builtin(resolved) &&
        declaration &&
        context.builtin(declaration) &&
        declaration.parent?.name &&
        text(declaration.parent.name).endsWith("ErrorConstructor")
      )
        return;
      const memberCall = [
        K.PropertyAccessExpression,
        K.ElementAccessExpression,
      ].includes(callee.kind);
      const candidate = resolved || (memberCall ? declaration : undefined);
      let target =
        candidate && bodies.has(key(candidate)) ? candidate : undefined;
      if (!target && candidate) {
        const symbol = candidate.name
          ? context.symbol(candidate.name)
          : context.symbol(candidate);
        target = context.declarations(symbol).find((d) => bodies.has(key(d)));
      }
      if (target) {
        if (
          memberCall ||
          [K.MethodDeclaration, K.GetAccessor, K.SetAccessor].includes(
            target.kind,
          )
        )
          unknown(owner, n, "member_dispatch_may_be_overridden");
        if (target.asteriskToken) {
          unknown(owner, n, "generator_body_is_not_executed_by_call");
          return;
        }
        const returned =
          signature && checker.getReturnTypeOfSignature(signature);
        if (
          returned &&
          context.promised(returned) &&
          !uses.some((u) => u.awaited || u.returned)
        )
          unknown(owner, n, "promise_use_not_resolved_to_await_or_return");
        for (const usage of uses)
          facts.edges.push({
            caller: owner,
            callee: key(target),
            location: location(n),
            ...usage,
          });
      } else
        unknown(
          owner,
          n,
          declaration
            ? "implementation_outside_scan_or_dynamic_signature"
            : "unresolved_call",
        );
    });
  }
  const channels = ["throw", "rejection", "result"];
  for (const candidate of facts.candidates) {
    const declaration = candidateDeclarations.get(candidate);
    candidate.flows = channels.filter(channel => errorDeclarations.some(([declarations, found]) => found === channel && declarations.has(declaration)));
  }
  return facts;
}

// Version-specific compiler access. Compiler handles never cross the JSON boundary.
import { createRequire } from "node:module";
import { readFileSync, realpathSync } from "node:fs";
import { join, relative, isAbsolute } from "node:path";
import { pathToFileURL } from "node:url";

export async function openCompiler(packageDir, project) {
  const metadata = JSON.parse(
    readFileSync(join(packageDir, "package.json"), "utf8"),
  );
  const major = Number(metadata.version.split(".")[0]);
  const require = createRequire(join(packageDir, "package.json"));
  let ast,
    checker,
    program,
    close = () => {},
    diagnostics,
    options;
  if (major === 6) {
    ast = require(packageDir);
    for (const method of [
      "readConfigFile",
      "parseJsonConfigFileContent",
      "createProgram",
      "getPreEmitDiagnostics",
    ])
      if (typeof ast[method] !== "function")
        throw new Error(`TypeScript ${metadata.version} lacks ${method}`);
    const loaded = ast.readConfigFile(project, ast.sys.readFile);
    if (loaded.error)
      return {
        version: metadata.version,
        diagnostics: [diagnostic(loaded.error)],
        close,
      };
    const parsed = ast.parseJsonConfigFileContent(
      loaded.config,
      ast.sys,
      join(project, ".."),
      undefined,
      project,
    );
    options = parsed.options;
    program = ast.createProgram({
      rootNames: parsed.fileNames,
      options,
      projectReferences: parsed.projectReferences,
    });
    checker = program.getTypeChecker();
    diagnostics = [...parsed.errors, ...ast.getPreEmitDiagnostics(program)];
  } else if (major === 7) {
    const { API } = await import(
      pathToFileURL(require.resolve(`${metadata.name}/unstable/sync`)).href
    );
    ast = await import(
      pathToFileURL(require.resolve(`${metadata.name}/unstable/ast`)).href
    );
    const api = new API({ cwd: join(project, "..") });
    let snapshot;
    close = () => {
      try {
        snapshot?.dispose();
      } finally {
        api.close();
      }
    };
    try {
      snapshot = api.updateSnapshot({ openProjects: [project] });
      const selected = snapshot.getProject(project);
      if (!selected)
        throw new Error("TypeScript API did not open the requested project");
      program = selected.program;
      checker = selected.checker;
      options = selected.compilerOptions;
      diagnostics = [
        ...program.getConfigFileParsingDiagnostics(),
        ...program.getProgramDiagnostics(),
        ...program.getGlobalDiagnostics(),
        ...program.getSyntacticDiagnostics(),
        ...program.getSemanticDiagnostics(),
      ];
    } catch (error) {
      close();
      throw error;
    }
  } else
    throw new Error(
      `Unsupported TypeScript ${metadata.version}; expected major 6 or 7`,
    );
  try {
    if (
      !checker ||
      typeof checker.getDeclaredTypeOfSymbol !== "function" ||
      typeof checker.getResolvedSignature !== "function"
    ) {
      throw new Error("TypeScript API lacks required checker capabilities");
    }
    // The native AST API exports flags separately from the checker API.
    const constants =
      major === 6
        ? ast
        : await import(
            pathToFileURL(require.resolve(`${metadata.name}/unstable/sync`))
              .href
          );
    const files =
      major === 6
        ? program.getSourceFiles()
        : program
            .getSourceFileNames()
            .map((file) => program.getSourceFile(file));
    for (const value of diagnostics) {
      if (!Number.isInteger(value.code) || !Number.isInteger(value.category))
        throw new Error("Unsupported compiler diagnostic format");
    }
    const context = {
      major,
      version: metadata.version,
      ast,
      constants,
      checker,
      program,
      files: files.filter(Boolean),
      close,
      diagnostics: diagnostics
        .filter((d) => d.category === 1)
        .map((d) => diagnostic(d)),
      noCheck: !!options.noCheck,
    };
    context.resolve = (node) => (node?.resolve ? node.resolve() : node);
    context.declarations = (symbol) =>
      (symbol?.declarations || []).map(context.resolve).filter(Boolean);
    context.signature = (node) => checker.getResolvedSignature(node);
    context.declaration = (signature) =>
      context.resolve(signature?.declaration);
    context.symbol = (node) => checker.getSymbolAtLocation(node);
    context.unalias = (symbol) =>
      symbol?.flags & constants.SymbolFlags.Alias
        ? checker.getAliasedSymbol(symbol)
        : symbol;
    context.type = (node) => checker.getTypeAtLocation(node);
    context.typeString = (type) => checker.typeToString(type);
    context.members = (type) =>
      type.flags & constants.TypeFlags.Union
        ? major === 6
          ? type.types
          : type.getTypes()
        : [type];
    context.propertyType = (type, name) => {
      const symbol = checker.getPropertyOfType(type, name);
      return symbol
        ? checker.getTypeOfSymbolAtLocation(
            symbol,
            context.declarations(symbol)[0] || files[0],
          )
        : undefined;
    };
    // TS 7 has no getAwaitedType. Resolve fulfillment parameter types through its public
    // signature API instead of assuming that every thenable is named Promise<T>.
    const promisedMembers = (type) => {
      if (typeof checker.getPromisedTypeOfPromise === "function") {
        const promised = checker.getPromisedTypeOfPromise(type);
        return promised ? context.members(promised) : undefined;
      }
      if (
        type.flags &
        (constants.TypeFlags.Any |
          constants.TypeFlags.Unknown |
          constants.TypeFlags.Never)
      )
        return;
      const then = context.propertyType(type, "then");
      if (!then || then.flags & constants.TypeFlags.Any) return;
      const values = [];
      for (const signature of checker.getSignaturesOfType(
        then,
        constants.SignatureKind.Call,
      )) {
        const first = signature.getParameters()[0];
        if (!first) continue;
        for (const callback of context.members(
          checker.getTypeOfSymbol(first),
        )) {
          for (const fulfilled of checker.getSignaturesOfType(
            callback,
            constants.SignatureKind.Call,
          )) {
            const value = fulfilled.getParameters()[0];
            if (value) values.push(checker.getTypeOfSymbol(value));
          }
        }
      }
      return values.length ? values : undefined;
    };
    context.promised = (type) =>
      context.members(type).some((member) => !!promisedMembers(member));
    context.awaitedMembers = (type, seen = new Set()) => {
      if (!type)
        throw new Error("Compiler did not return a function return type");
      if (seen.has(type))
        throw new Error("Recursive thenable cannot be resolved");
      const next = new Set(seen).add(type);
      return context.members(type).flatMap((member) => {
        const promised = promisedMembers(member);
        return promised
          ? promised.flatMap((value) => context.awaitedMembers(value, next))
          : [member];
      });
    };
    context.builtin = (node) => {
      const file = node.getSourceFile();
      return file.isDeclarationFile && program.isSourceFileDefaultLibrary(file);
    };
    return context;
  } catch (error) {
    close();
    throw error;
  }
}

function diagnostic(value) {
  const file = value.file?.fileName ?? value.fileName;
  // The native API uses text/messageChain and UTF-16 pos/end; the classic API
  // uses messageText/next and start/length. Both acquire text without its BOM.
  const nativeMessage = (value) => {
    if (typeof value.text !== "string")
      throw new Error("Unsupported compiler diagnostic message");
    return [value.text, ...(value.messageChain || []).map(nativeMessage)].join(
      "\n",
    );
  };
  const message =
    typeof value.text === "string"
      ? nativeMessage(value)
      : flatten(value.messageText);
  const result = { code: `TS${value.code}`, message };
  if (file) {
    result.file = file;
    const start = value.start ?? value.pos;
    if (Number.isInteger(start)) {
      const original = readFileSync(file, "utf8");
      const text = original.startsWith("\uFEFF") ? original.slice(1) : original;
      result.byteOffset = Buffer.byteLength(text.slice(0, start));
      result.line = text.slice(0, start).split("\n").length;
    }
  }
  return result;
}
function flatten(value) {
  if (typeof value === "string") return value;
  if (!value || typeof value.messageText !== "string")
    throw new Error("Unsupported compiler diagnostic message");
  return [value.messageText, ...(value.next || []).map(flatten)].join("\n");
}
export const order = (a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b));
export const sorted = (values) => [...new Set(values)].sort(order);

export function syntax(context, root) {
  const { SyntaxKind: K, NodeFlags, ModifierFlags } = context.ast;
  const sources = new Map();
  const source = (file) => {
    if (!sources.has(file)) {
      const original = readFileSync(file.fileName, "utf8");
      if (
        original !== file.text &&
        !(original.startsWith("\uFEFF") && original.slice(1) === file.text)
      )
        throw new Error(`Source changed while analyzing ${file.fileName}`);
      sources.set(file, { original, prefix: original === file.text ? 0 : 1 });
    }
    return sources.get(file);
  };
  const location = (node) => {
    const file = node.getSourceFile(),
      { original, prefix } = source(file);
    // tsrs strips the UTF-8 BOM before parsing; preserve that coordinate convention.
    const content = original.startsWith("\uFEFF")
      ? original.slice(1)
      : original;
    const offset =
      node.getStart(file) + prefix - (original.startsWith("\uFEFF") ? 1 : 0);
    return {
      file: relative(root, file.fileName).replaceAll("\\", "/"),
      byteOffset: Buffer.byteLength(content.slice(0, offset)),
      line: content.slice(0, offset).split("\n").length,
    };
  };
  const text = (node) =>
    [K.Identifier, K.PrivateIdentifier, K.StringLiteral].includes(node.kind)
      ? node.text
      : node.getText(node.getSourceFile()).trim();
  const names = [
    "SourceFile",
    "FunctionDeclaration",
    "FunctionExpression",
    "ArrowFunction",
    "MethodDeclaration",
    "Constructor",
    "GetAccessor",
    "SetAccessor",
    "ThrowStatement",
    "CallExpression",
    "NewExpression",
  ];
  const kind = (node) =>
    names.find((name) => node.kind === K[name]) || K[node.kind];
  const key = (node) => {
    const loc = location(node);
    return `${loc.file}:${loc.byteOffset}:${kind(node)}`;
  };
  const functionLike = (node) =>
    [
      K.FunctionDeclaration,
      K.FunctionExpression,
      K.ArrowFunction,
      K.MethodDeclaration,
      K.Constructor,
      K.GetAccessor,
      K.SetAccessor,
    ].includes(node.kind);
  const walk = (node, visit) => {
    visit(node);
    node.forEachChild((child) => {
      walk(child, visit);
    });
  };
  const walkBody = (node, visit) => {
    if (functionLike(node)) return;
    visit(node);
    node.forEachChild((child) => {
      walkBody(child, visit);
    });
  };
  const modifier = (node, flag) =>
    !!node.modifiers?.some(
      (m) => flag === ModifierFlags.Async && m.kind === K.AsyncKeyword,
    );
  const local = (file) => {
    const name = relative(root, file.fileName).replaceAll("\\", "/");
    return (
      !name.startsWith("../") &&
      !isAbsolute(name) &&
      !name.split("/").includes("node_modules")
    );
  };
  // Confirm that files observed through the compiler still have their acquired contents.
  const verify = () => {
    for (const [file, acquired] of sources)
      if (readFileSync(file.fileName, "utf8") !== acquired.original)
        throw new Error(`Source changed while analyzing ${file.fileName}`);
  };
  // Tag-only requests also validate the compiler's acquired source text.
  for (const file of context.files) source(file);
  return {
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
    verify,
  };
}

export class TagError extends Error {}

export function tags(context, source) {
  const {
    checker,
    constants: { TypeFlags: T, SymbolFlags: S },
  } = context;
  const file = context.program.getSourceFile(realpathSync(source.file));
  if (!file)
    throw new TagError(`${source.file} is not in the TypeScript project`);
  const module = checker.getSymbolAtLocation(file);
  if (!module)
    throw new TagError("Source must be a module with an exported error type");
  const symbol = context.unalias(
    checker.getExportsOfModule(module).find((s) => s.name === source.export),
  );
  if (!symbol)
    throw new TagError(`Export ${source.export} not found in ${source.file}`);
  if (!(symbol.flags & (S.TypeAlias | S.Interface | S.Class)))
    throw new TagError("Export must be a type alias, interface, or class");
  const values = [];
  function collect(type) {
    if (type.flags & T.Never) return;
    if (type.flags & T.Union) {
      for (const member of context.members(type)) collect(member);
      return;
    }
    if (type.flags & (T.Any | T.Unknown | T.TypeParameter))
      throw new TagError(`Cannot enumerate ${checker.typeToString(type)}`);
    const prop = checker.getPropertyOfType(type, source.discriminant);
    if (!prop)
      throw new TagError(
        `Missing discriminant ${JSON.stringify(source.discriminant)} in ${checker.typeToString(type)}`,
      );
    if (prop.flags & S.Optional)
      throw new TagError(
        `Discriminant ${JSON.stringify(source.discriminant)} is optional`,
      );
    const value = checker.getTypeOfSymbolAtLocation(
      prop,
      context.declarations(prop)[0] || file,
    );
    for (const member of context.members(value)) {
      if (!(member.flags & T.StringLiteral))
        throw new TagError(
          `Discriminant must be a finite union of string literals; found ${checker.typeToString(member)}`,
        );
      values.push(member.value);
    }
  }
  collect(checker.getDeclaredTypeOfSymbol(symbol));
  return sorted(values);
}

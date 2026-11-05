// Compiler acquisition for mutant checks. Sources are overlaid, never written to disk.
import { createRequire } from "node:module";
import { readFileSync, existsSync, realpathSync } from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { pathToFileURL } from "node:url";
import { byteIndices } from "./ranges.mjs";

function packageDirectory(root, explicit) {
  if (explicit) return realpathSync(explicit);
  for (let current = root; ; current = dirname(current)) {
    const candidate = join(current, "node_modules/typescript");
    if (existsSync(join(candidate, "package.json")))
      return realpathSync(candidate);
    if (dirname(current) === current) break;
  }
  throw new Error("Cannot find TypeScript; use --typecheck-package");
}

async function open(root, project, packageDir) {
  const metadata = JSON.parse(
    readFileSync(join(packageDir, "package.json"), "utf8"),
  );
  const load = createRequire(join(packageDir, "package.json"));
  const overrides = new Map(),
    versions = new Map();
  let compiler, api, snapshot, service, parsed;
  const close = () => {
    try {
      service?.dispose();
      snapshot?.dispose();
    } finally {
      api?.close();
    }
  };
  try {
    const major = Number(metadata.version.split(".")[0]);
    if (major <= 6 && major > 0) {
      compiler = load(packageDir);
      parsed = compiler.getParsedCommandLineOfConfigFile(
        project,
        {},
        {
          ...compiler.sys,
          onUnRecoverableConfigFileDiagnostic: (diagnostic) => {
            throw new Error(
              compiler.flattenDiagnosticMessageText(
                diagnostic.messageText,
                "\n",
              ),
            );
          },
        },
      );
      if (!parsed || parsed.errors.length)
        throw new Error(
          "Cannot parse typecheck project: " +
            JSON.stringify(parsed?.errors.map((d) => d.messageText)),
        );
      const host = {
        getCompilationSettings: () => parsed.options,
        getScriptFileNames: () => parsed.fileNames,
        getScriptVersion: (file) => String(versions.get(file) || 0),
        getScriptSnapshot: (file) => {
          const text = overrides.get(file) ?? compiler.sys.readFile(file);
          return text === undefined
            ? undefined
            : compiler.ScriptSnapshot.fromString(text);
        },
        getCurrentDirectory: () => dirname(project),
        getDefaultLibFileName: (options) =>
          compiler.getDefaultLibFilePath(options),
        fileExists: (file) =>
          overrides.has(file) || compiler.sys.fileExists(file),
        readFile: (file) => overrides.get(file) ?? compiler.sys.readFile(file),
        readDirectory: compiler.sys.readDirectory,
        directoryExists: compiler.sys.directoryExists,
        getDirectories: compiler.sys.getDirectories,
      };
      service = compiler.createLanguageService(
        host,
        compiler.createDocumentRegistry(),
      );
    } else if (major === 7) {
      compiler = await import(
        pathToFileURL(load.resolve(`${metadata.name}/unstable/sync`)).href
      );
      api = new compiler.API({
        cwd: root,
        fs: { readFile: (file) => overrides.get(file) },
      });
      snapshot = api.updateSnapshot({ openProjects: [project] });
    } else
      throw new Error(
        `Unsupported mutant typechecker: TypeScript ${metadata.version}`,
      );
    const context = () => {
      if (api) {
        const selected = snapshot.getProject(project);
        if (!selected)
          throw new Error("TypeScript API did not open the requested project");
        return {
          program: selected.program,
          checker: selected.checker,
          options: selected.compilerOptions,
        };
      }
      const program = service.getProgram();
      if (!program)
        throw new Error("TypeScript did not create a language-service program");
      return {
        program,
        checker: program.getTypeChecker(),
        options: parsed.options,
      };
    };
    if (context().options.noCheck)
      throw new Error("noCheck must be false for mutant typechecking");
    const diagnose = () => {
      const { program } = context();
      const diagnostics = api
        ? [
            ...program.getConfigFileParsingDiagnostics(),
            ...program.getProgramDiagnostics(),
            ...program.getGlobalDiagnostics(),
            ...program.getSyntacticDiagnostics(),
            ...program.getSemanticDiagnostics(),
          ]
        : compiler.getPreEmitDiagnostics(program);
      return diagnostics
        .filter((d) => d.category === 1)
        .map((d) => {
          const message = api
            ? nativeMessage(d)
            : compiler.flattenDiagnosticMessageText(d.messageText, "\n");
          const file = d.file?.fileName ?? d.fileName;
          return `${file ? relative(root, file) : "<project>"}: TS${d.code} ${message}`;
        });
    };
    const change = (file, text) => {
      if (text === undefined) overrides.delete(file);
      else overrides.set(file, text);
      versions.set(file, (versions.get(file) || 0) + 1);
      if (api) {
        const next = api.updateSnapshot({ fileChanges: { changed: [file] } });
        snapshot.dispose();
        snapshot = next;
      }
    };
    return {
      version: metadata.version,
      compiler,
      context,
      diagnose,
      change,
      close,
    };
  } catch (error) {
    close();
    throw error;
  }
}

function nativeMessage(diagnostic) {
  if (!Number.isInteger(diagnostic.code) || typeof diagnostic.text !== "string")
    throw new Error("Unsupported TypeScript diagnostic format");
  return [
    diagnostic.text,
    ...(diagnostic.messageChain || []).map(nativeMessage),
  ].join("\n");
}

// Ignore positions, which shift after a replacement; retain file, code, message and multiplicity.
export function newDiagnostic(before, after) {
  const counts = new Map();
  for (const message of before)
    counts.set(message, (counts.get(message) || 0) + 1);
  for (const message of after) {
    const count = counts.get(message) || 0;
    if (!count) return message;
    counts.set(message, count - 1);
  }
}

export function applyCandidate(source, candidate) {
  const indices = byteIndices(source),
    start = indices.get(candidate.byteOffset),
    end = indices.get(candidate.end);
  if (
    start === undefined ||
    end === undefined ||
    start > end ||
    source.slice(start, end) !== candidate.original
  )
    throw new Error(
      `Invalid original text or UTF-8 range for ${candidate.key}`,
    );
  return source.slice(0, start) + candidate.replacement + source.slice(end);
}

function equivalents(checker, file, source, candidates) {
  const { program, checker: types, options } = checker.context(),
    T = checker.compiler.TypeFlags;
  if (
    !options.noUncheckedIndexedAccess ||
    types.isTypeAssignableTo(types.getUndefinedType(), types.getStringType())
  )
    return new Map();
  const ast = program.getSourceFile(file);
  if (!ast) throw new Error(`Typecheck source is outside the project: ${file}`);
  const chains = new Map();
  const visit = (node) => {
    if (node.questionDotToken && node.expression)
      chains.set(node.questionDotToken.getStart(ast), node.expression);
    node.forEachChild((child) => {
      visit(child);
    });
  };
  visit(ast);
  const primitive = [
    "String",
    "Number",
    "BigInt",
    "Boolean",
    "ESSymbol",
    "StringLiteral",
    "NumberLiteral",
    "BigIntLiteral",
    "BooleanLiteral",
    "UniqueESSymbol",
    "EnumLiteral",
    "Enum",
    "NonPrimitive",
    "Object",
    "TemplateLiteral",
    "StringMapping",
  ].reduce((mask, name) => mask | T[name], 0);
  const excludesNull = (type) => {
    if (type.flags & (T.Union | T.Intersection)) {
      const members = type.getTypes ? type.getTypes() : type.types;
      return type.flags & T.Union
        ? members.every(excludesNull)
        : members.some(excludesNull);
    }
    return !!(type.flags & primitive);
  };
  const result = new Map(),
    indices = byteIndices(source);
  for (const candidate of candidates.filter(
    (c) => c.mutator === "OptionalChaining",
  )) {
    let prefix = 0;
    while (
      prefix < candidate.original.length &&
      candidate.original[prefix] === candidate.replacement[prefix]
    )
      prefix++;
    if (candidate.original.slice(prefix, prefix + 2) !== "?.") continue;
    const target = chains.get(
      indices.get(candidate.byteOffset) +
        prefix -
        (source.startsWith("\uFEFF") ? 1 : 0),
    );
    if (target && excludesNull(types.getTypeAtLocation(target)))
      result.set(
        candidate.key,
        `equivalent: ${target.getText(ast)} excludes null and undefined`,
      );
  }
  return result;
}

export async function typecheckInventory(
  options,
  inventory,
  progress = () => {},
) {
  const root = options.root;
  const project = realpathSync(
    options.typecheckProject || join(root, "tsconfig.json"),
  );
  const packageDir = packageDirectory(root, options.typecheckPackage);
  const checker = await open(root, project, packageDir);
  try {
    const baseline = checker.diagnose(),
      outcomes = [];
    for (const [file, entry] of inventory) {
      if (!/\.[cm]?tsx?$/.test(file)) continue;
      const candidates = entry.candidates.filter((c) => !c.ignored);
      if (!candidates.length) continue;
      if (!checker.context().program.getSourceFile(file))
        throw new Error(`Typecheck source is outside the project: ${file}`);
      const equivalent = equivalents(checker, file, entry.source, candidates);
      for (const candidate of candidates) {
        const equivalentReason = equivalent.get(candidate.key) ?? null;
        let diagnostic = null;
        if (!equivalentReason) {
          try {
            checker.change(file, applyCandidate(entry.source, candidate));
            diagnostic = newDiagnostic(baseline, checker.diagnose()) ?? null;
          } finally {
            checker.change(file, undefined);
          }
        }
        outcomes.push({ key: candidate.key, diagnostic, equivalentReason });
        progress(candidate.key);
      }
    }
    return {
      info: {
        version: checker.version,
        package: packageDir,
        project,
        checked: outcomes.length,
        compileErrors: outcomes.filter((r) => r.diagnostic !== null)
          .length,
        equivalent: outcomes.filter((r) => r.equivalentReason !== null).length,
      },
      outcomes,
    };
  } finally {
    checker.close();
  }
}

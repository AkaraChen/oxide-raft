// Import graph of the pinned upstream packages, from the TypeScript AST.
// Usage: node tools/scope/graph.mjs > docs/migration/scope/graph.json
//
// Nodes are files relative to the repo root, always with `/` separators.
// Edges cover static imports, `export … from`, `import type`, dynamic
// `import("…")`, `require("…")`, and `vi.mock/doMock/importActual("…")` module
// ids (kind "import"), plus files a module names by path (kind "path"):
// `new URL("<lit>", import.meta.url)` and `join`/`resolve`/`path.join`/
// `path.resolve` calls whose arguments are all string literals or
// `import.meta.dirname`/`__dirname`. Workspace package ids resolve to their
// source entry. Test files are `*.test.ts` and `*.sea-suite.ts`.
import { readdirSync, readFileSync, existsSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const upstream = join(root, "upstream/raft-source");
const require = createRequire(join(upstream, "package.json"));
const ts = require("typescript");

const packageDirs = {
  "@botiverse/raft": "upstream/raft-source/packages/cli",
  "@botiverse/raft-computer": "upstream/raft-source/packages/computer",
  "@botiverse/raft-daemon": "upstream/raft-source/packages/daemon",
  "@botiverse/raft-shared": "upstream/raft-source/packages/shared",
  "@botiverse/raft-trace-client": "upstream/raft-source/packages/trace-client",
  "@botiverse/raft-sync-core": "upstream/raft-source/packages/sync-core",
  "@botiverse/oar": "upstream/oar/packages/oar",
};

const posix = (p) => p.split(sep).join("/");
const rel = (file) => posix(relative(root, file));

function walk(dir, out = []) {
  if (!existsSync(dir)) return out;
  const entries = readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  for (const entry of entries) {
    if (entry.name === "node_modules" || entry.name === "dist") continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else if (/\.(ts|tsx|mts)$/.test(entry.name) && !entry.name.endsWith(".d.ts")) out.push(full);
  }
  return out;
}

const scanRoots = [
  "upstream/raft-source/packages/cli",
  "upstream/raft-source/packages/computer",
  "upstream/raft-source/packages/daemon",
  "upstream/raft-source/packages/shared",
  "upstream/raft-source/packages/trace-client",
  "upstream/oar/packages/oar",
  "upstream/oar/packages/cli",
  "upstream/oar/tests",
  "upstream/oar/sea-trial/vendor",
  "upstream/oar/apps/coxswain/test",
];

// Maps a package subpath ("." or "./core") to its source file through the
// exports map, reading built paths (dist/*.js, dist/*.d.ts) back as src/*.ts.
function packageSubpath(pkgDir, subpath) {
  const pkg = JSON.parse(readFileSync(join(root, pkgDir, "package.json"), "utf8"));
  const target = pkg.exports?.[subpath];
  const fromExports = typeof target === "string" ? [target] : [target?.source, target?.types, target?.import, target?.default];
  const fallback = subpath === "." ? [pkg.source, pkg.main, "src/index.ts"] : [`src/${subpath.slice(2)}`, subpath.slice(2)];
  for (const candidate of [...fromExports, ...fallback].filter((c) => typeof c === "string")) {
    const sourcePath = candidate.replace(/^\.\//, "").replace(/^dist\//, "src/").replace(/\.d\.ts$/, ".ts");
    const file = resolveFile(join(root, pkgDir, sourcePath));
    if (file) return file;
  }
  return null;
}

function resolveFile(base) {
  const noJs = base.replace(/\.(m?js)$/, "");
  const tries = [base, `${noJs}.ts`, `${noJs}.tsx`, `${noJs}.mts`, join(noJs, "index.ts")];
  for (const t of tries) {
    if (existsSync(t) && statSync(t).isFile() && /\.(ts|tsx|mts)$/.test(t)) return t;
  }
  return null;
}

function resolveSpecifier(fromFile, spec) {
  if (spec.startsWith(".")) return resolveFile(resolve(dirname(fromFile), spec));
  for (const [id, dir] of Object.entries(packageDirs)) {
    if (spec === id) return packageSubpath(dir, ".");
    if (spec.startsWith(`${id}/`)) return packageSubpath(dir, `./${spec.slice(id.length + 1)}`);
  }
  return null;
}

const DIR_TOKENS = new Set(["__dirname"]);
const isMetaProp = (node, name) =>
  ts.isPropertyAccessExpression(node) && ts.isMetaProperty(node.expression) && node.name.text === name;

// Literal path arguments of join/resolve: every argument is a string literal
// or the module's own directory. Returns the joined relative path or null.
function literalPathCall(node, file) {
  const callee = node.expression;
  const name = ts.isIdentifier(callee) ? callee.text : ts.isPropertyAccessExpression(callee) ? callee.name.text : null;
  if (name !== "join" && name !== "resolve") return null;
  if (ts.isPropertyAccessExpression(callee) && !(ts.isIdentifier(callee.expression) && /^(path|posix|win32)$/.test(callee.expression.text))) return null;
  const [first, ...rest] = node.arguments;
  if (!first || !(isMetaProp(first, "dirname") || (ts.isIdentifier(first) && DIR_TOKENS.has(first.text)))) return null;
  if (rest.length === 0 || !rest.every((a) => ts.isStringLiteralLike(a))) return null;
  if (!/\.(m?[jt]sx?)$/.test(rest.at(-1).text)) return null;
  return resolve(dirname(file), ...rest.map((a) => a.text));
}

function specifiers(file) {
  const text = readFileSync(file, "utf8");
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const found = [];
  const visit = (node) => {
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) {
      found.push({ spec: node.moduleSpecifier.text, typeOnly: Boolean(node.importClause?.isTypeOnly || node.isTypeOnly), kind: "import" });
    } else if (ts.isCallExpression(node) && node.arguments.length > 0 && ts.isStringLiteralLike(node.arguments[0])) {
      const callee = node.expression;
      const isDynamicImport = callee.kind === ts.SyntaxKind.ImportKeyword;
      const isRequire = ts.isIdentifier(callee) && callee.text === "require";
      const isMock = ts.isPropertyAccessExpression(callee) && ts.isIdentifier(callee.expression) && callee.expression.text === "vi" && /^(mock|doMock|importActual)$/.test(callee.name.text);
      if (isDynamicImport || isRequire || isMock) found.push({ spec: node.arguments[0].text, typeOnly: false, kind: "import" });
    } else if (ts.isImportTypeNode(node) && ts.isLiteralTypeNode(node.argument) && ts.isStringLiteral(node.argument.literal)) {
      found.push({ spec: node.argument.literal.text, typeOnly: true, kind: "import" });
    }
    if (ts.isNewExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "URL" && node.arguments?.length === 2
      && ts.isStringLiteralLike(node.arguments[0]) && isMetaProp(node.arguments[1], "url") && /^\..*\.(m?[jt]sx?)$/.test(node.arguments[0].text)) {
      found.push({ spec: node.arguments[0].text, typeOnly: false, kind: "path" });
    }
    if (ts.isCallExpression(node)) {
      const target = literalPathCall(node, file);
      if (target) found.push({ abs: target, typeOnly: false, kind: "path" });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { found, lines: text.split("\n").length };
}

const isTestFile = (file) => /\.(test\.tsx?|sea-suite\.ts)$/.test(file);

const nodes = {};
for (const scanRoot of scanRoots) {
  for (const file of walk(join(root, scanRoot))) {
    const { found, lines } = specifiers(file);
    const edges = [];
    const seen = new Set();
    const external = new Set();
    for (const { spec, abs, typeOnly, kind } of found) {
      const target = abs ? resolveFile(abs) : resolveSpecifier(file, spec);
      if (target) {
        const to = rel(target);
        const id = `${to}\u0000${typeOnly}\u0000${kind}`;
        if (!seen.has(id)) edges.push({ to, typeOnly, ...(kind === "path" ? { kind } : {}) });
        seen.add(id);
      } else if (spec && kind === "import") external.add(spec);
    }
    nodes[rel(file)] = { lines, test: isTestFile(file), edges, external: [...external].sort() };
  }
}

const sorted = Object.fromEntries(Object.entries(nodes).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
process.stdout.write(`${JSON.stringify({ nodes: sorted }, null, 1)}\n`);

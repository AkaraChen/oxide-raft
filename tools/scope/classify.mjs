// Classifies every upstream file as in scope, dropped, or out of scope, and
// every upstream test file as port, waive, or out of scope.
// Usage: node tools/scope/classify.mjs  (writes docs/migration/scope/files.json)
//
// Only `import` edges decide scope and a test's status; `path` edges (files a
// test spawns or reads by path) only pull fixtures in as test support.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const scopeDir = join(root, "docs/migration/scope");
const { nodes } = JSON.parse(readFileSync(join(scopeDir, "graph.json"), "utf8"));
const rules = JSON.parse(readFileSync(join(scopeDir, "rules.json"), "utf8"));

const globToRegex = (pattern) =>
  new RegExp(`^${pattern.replace(/[.+^${}()|\\]/g, "\\$&").replace(/\*/g, "[^/]*")}$`);
const dropRules = rules.drops.map((d) => ({ ...d, re: globToRegex(d.pattern) }));
const dropReason = (file) => dropRules.find((d) => d.re.test(file))?.reason;

const roots = [
  ...rules.roots,
  ...Object.entries(rules.closures).flatMap(([prefix, closure]) => closure.files.map((f) => prefix + f)),
];
for (const r of roots) if (!nodes[r]) throw new Error(`root not in graph: ${r}`);

// Walk source edges from the roots, never entering a dropped file and never
// following a barrel's re-exports.
const inScope = new Set();
const droppedReached = new Map();
const queue = [...roots];
while (queue.length > 0) {
  const file = queue.shift();
  if (inScope.has(file)) continue;
  inScope.add(file);
  if (rules.barrels[file] || rules.partial?.[file]) continue;
  for (const { to, kind } of nodes[file].edges) {
    if (kind === "path" || nodes[to]?.test) continue;
    const reason = dropReason(to);
    if (reason) {
      if (!droppedReached.has(to)) droppedReached.set(to, reason);
      continue;
    }
    if (!inScope.has(to)) queue.push(to);
  }
}

const packageOf = (file) => {
  const m = file.match(/^upstream\/(?:raft-source\/packages\/([^/]+)|(oar))\//);
  return m ? m[1] ?? "oar" : "other";
};

const sources = {};
for (const [file, node] of Object.entries(nodes)) {
  if (node.test) continue;
  const reason = dropReason(file);
  sources[file] = {
    package: packageOf(file),
    lines: node.lines,
    status: inScope.has(file) ? "in-scope" : reason ? "dropped" : "out-of-scope",
    ...(reason ? { reason } : {}),
  };
}

// A test's subject is the file it is named after: a.test.ts, a.variant.test.ts → a.ts.
function subjectOf(testFile) {
  if (rules.subjects?.[testFile]) return sources[rules.subjects[testFile]] ? rules.subjects[testFile] : null;
  const base = testFile.replace(/\.test\.tsx?$/, "");
  const parts = base.split("/");
  const name = parts.pop();
  const segments = name.split(".");
  for (let n = segments.length; n >= 1; n -= 1) {
    const candidate = [...parts, segments.slice(0, n).join(".")].join("/") + ".ts";
    if (sources[candidate]) return candidate;
  }
  return null;
}

const testWaivers = new Map((rules.testWaivers ?? []).map((w) => [w.file, w]));
const outOfScopeTests = (rules.outOfScopeTests ?? []).map((o) => ({ ...o, re: globToRegex(o.pattern) }));
const partialTests = rules.partialTests ?? {};
const inScopeLike = (s) => s === "in-scope" || s === "test-support";

function classifyTest(file, node) {
  const imports = [...new Set(node.edges.filter((e) => e.kind !== "path").map((e) => e.to).filter((t) => sources[t]))];
  const statuses = imports.map((t) => sources[t].status);
  const droppedImports = imports.filter((t) => sources[t].status === "dropped");
  const subject = subjectOf(file);
  const unscannedSubject = rules.subjects?.[file] && !subject;
  const waiver = testWaivers.get(file);
  const outOfScopeRule = outOfScopeTests.find((o) => o.re.test(file));
  let status;
  let reason;
  if (outOfScopeRule) {
    status = "out-of-scope";
    reason = outOfScopeRule.reason;
  } else if (unscannedSubject) {
    status = "out-of-scope";
  } else if (waiver) {
    status = "waive";
    reason = `${waiver.category}: ${waiver.reason}`;
  } else if (subject && sources[subject].status === "dropped") {
    status = "waive";
    reason = sources[subject].reason;
  } else if (subject && inScopeLike(sources[subject].status)) {
    status = droppedImports.length > 0 ? "port-partial" : "port";
  } else if (subject) {
    status = "out-of-scope";
  } else if (imports.length === 0 || statuses.every(inScopeLike)) {
    status = "port";
  } else if (droppedImports.length > 0 && !statuses.some(inScopeLike)) {
    status = "waive";
    reason = sources[droppedImports[0]].reason;
  } else if (statuses.some(inScopeLike)) {
    status = droppedImports.length > 0 ? "port-partial" : "port";
  } else {
    status = "out-of-scope";
  }
  if (partialTests[file]) {
    if (status !== "port" && status !== "port-partial") throw new Error(`partialTests entry is ${status}, not port: ${file}`);
    status = "port-partial";
    reason = partialTests[file];
  }
  return {
    package: packageOf(file),
    lines: node.lines,
    subject,
    status,
    ...(reason ? { reason } : {}),
    ...(status === "out-of-scope" && subject && rules.outOfScopeNotes?.[subject] ? { note: rules.outOfScopeNotes[subject] } : {}),
    ...(droppedImports.length > 0 ? { droppedImports } : {}),
    ...((status === "port" || status === "port-partial") && imports.some((t) => sources[t].status === "out-of-scope")
      ? { needsOutOfScope: imports.filter((t) => sources[t].status === "out-of-scope") }
      : {}),
  };
}

const testEntries = Object.entries(nodes).filter(([, n]) => n.test);
let tests = Object.fromEntries(testEntries.map(([f, n]) => [f, classifyTest(f, n)]));

// Files that ported tests need but no product entry reaches are ported as
// test support. A file that another test is named after is never pulled in
// this way: that would drag an out-of-scope module in through its own test.
const testSubjects = new Set(Object.values(tests).map((t) => t.subject).filter(Boolean));
const supportQueue = [
  ...(rules.testSupport ?? []).map((file) => ({ file, from: null })),
  ...testEntries
    .filter(([f]) => tests[f].status === "port" || tests[f].status === "port-partial")
    .flatMap(([f, n]) => n.edges.map((e) => ({ file: e.to, from: f }))),
];
while (supportQueue.length > 0) {
  const { file, from } = supportQueue.shift();
  const s = sources[file];
  if (!s || s.status !== "out-of-scope") continue;
  if (rules.outOfScopeNotes?.[file]) continue;
  if (from !== null && Object.keys(rules.closures).some((prefix) => file.startsWith(prefix))) continue;
  if (from && testSubjects.has(file) && tests[from]?.subject === file) continue;
  s.status = "test-support";
  supportQueue.push(...nodes[file].edges.map((e) => ({ file: e.to, from: null })));
}
tests = Object.fromEntries(testEntries.map(([f, n]) => [f, classifyTest(f, n)]));

// Waived test files the TS graph does not scan (release tooling in .mjs).
const WAIVER_CATEGORIES = new Set(["dropped", "source-scan", "packaging", "node-preflight"]);
for (const w of rules.testWaivers ?? []) {
  if (!WAIVER_CATEGORIES.has(w.category)) throw new Error(`testWaivers category ${w.category} is not a mapping-guide §12 category: ${w.file}`);
  if (!existsSync(join(root, w.file))) throw new Error(`testWaivers file does not exist: ${w.file}`);
  if (!tests[w.file]) tests[w.file] = { package: packageOf(w.file), lines: 0, subject: null, status: "waive", reason: `${w.category}: ${w.reason}` };
}

writeFileSync(join(scopeDir, "files.json"), `${JSON.stringify({ sources, tests }, null, 1)}\n`);

const summary = {};
const bump = (pkg, key, lines) => {
  summary[pkg] ??= {};
  summary[pkg][key] ??= { files: 0, lines: 0 };
  summary[pkg][key].files += 1;
  summary[pkg][key].lines += lines;
};
for (const s of Object.values(sources)) bump(s.package, `src:${s.status}`, s.lines);
for (const t of Object.values(tests)) bump(t.package, `test:${t.status}`, t.lines);
for (const [pkg, rows] of Object.entries(summary)) {
  console.log(pkg);
  for (const [key, { files, lines }] of Object.entries(rows).sort()) console.log(`  ${key.padEnd(22)} ${String(files).padStart(4)} files ${String(lines).padStart(7)} lines`);
}
for (const file of Object.keys(partialTests)) if (!tests[file]) throw new Error(`partialTests file not in graph: ${file}`);
for (const o of outOfScopeTests) if (!Object.keys(tests).some((f) => o.re.test(f))) throw new Error(`outOfScopeTests pattern matches no test: ${o.pattern}`);
const unreachedDrops = dropRules.filter((d) => ![...droppedReached.keys()].some((f) => d.re.test(f)) && !Object.keys(sources).some((f) => d.re.test(f)));
if (unreachedDrops.length) console.log("drop patterns matching no file:", unreachedDrops.map((d) => d.pattern));

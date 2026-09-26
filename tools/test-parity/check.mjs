// Test parity gate (decisions.md D9, mapping-guide.md §1.6, §12, GOAL Gate 5).
// Usage: node tools/test-parity/check.mjs [--os macos] [--package cli] [--missing] [--write-file-waivers]
//        node tools/test-parity/check.mjs --name "<upstream title>"   (prints the §1.6 fn name)
//
// Upstream cases come from tests/parity/upstream-<os>.json (tools/test-list).
// A case is keyed by upstream file + describe path + title. It is ported when
// the Rust test file that mapping-guide §1.1/§1.6 assigns to its upstream file
// is compiled (reachable through `mod` / `#[path] mod` declarations from the
// crate's src/lib.rs, src/main.rs or src/bin/*.rs) and carries
// `// test: <key>` above a test function. It is waived when tests-waived.md
// lists it. All paths are `/`-separated on every OS.
//
// Rust test file for an upstream test file (mapping-guide §1.1, §1.6):
//   strip `.test.<ext>` (ts, tsx, mts, js, mjs), snake_case each path segment,
//   turn remaining `.`/`-` into `_`, append `_tests.rs`:
//   packages/daemon/src/agentProcessManager.claude.test.ts
//     → crates/raft-daemon-core/src/agent_process_manager_claude_tests.rs
//   oar's tests/ tree has no sibling sources; upstream/oar/tests/<p>.test.ts
//     → crates/oar/src/tests/<p>_tests.rs, compiled through `mod tests;` trees.
//
// Test fn names (§1.6), fixed here as the literal reading of the guide:
//   1. NFKD-normalize the title, drop combining marks U+0300–U+036F, lowercase.
//   2. Every run of characters outside [a-z0-9] (any other non-ASCII included:
//      `§`, `→`, `—`, CJK) becomes one `_`; leading and trailing runs too.
//      No other transliteration table (so `§10` → `_10`, not `ss10`).
//   3. Prefix `t_` when the result starts with a digit; truncate to 80 chars.
//   4. Dedupe per Rust test file (= per upstream test file) in oracle order over
//      every listed case of that file, ported or not: the 2nd case with the same
//      base is `<base>_2`, the 3rd `<base>_3`. Rust order and `mod` nesting do
//      not matter. A generated name equal to another case's name is reported.
//
// Gate 5 per crate: every compiled test fn (any `#[test]`, `#[tokio::test…]`,
// `#[rstest]`, `#[<path>::test…]`) is counted. Keyed ones are rust-tests and the
// gate needs rust-tests == ported == in-scope − waived. Test fns that port no
// upstream test are allowed only under EXTRA_TEST_LOCATIONS below (path prefixes,
// each citing the record that asks for them); they are counted in the `extra`
// column and left out of the equality. An unkeyed test fn anywhere else is a
// problem.
//
// tests-waived.md rows: key `*` (whole file, status waive), a case key
// (status port-partial), or `<case key> :: assert <what>` (status port or
// port-partial): the case is ported and counted as ported, and the row records
// one upstream assertion or input the Rust test leaves out, category `dropped`.
//
// Upstream skips (D9). vitest reports no skip reason, so docs/migration/scope/
// rules.json `upstreamSkips` gives each skipped in-scope case its kind, read from
// the upstream skip/skipIf expression (a skipped case without one is a problem):
//   platform — `process.platform` condition: needs
//     `#[cfg_attr(<cfg true here>, ignore = "upstream skip: …")]`; a case not
//     skipped here may carry one only for a cfg that is false here.
//   opt-in — env var gate (RUN_*_INTEGRATION_TESTS, …): no ignore attribute;
//     the fn body must name the var (`std::env::var("<VAR>")` → return early)
//     or carry `// upstream opt-in: <VAR>`. Applies whatever the listed outcome.
//   always — unconditional `test.skip`: `#[ignore = "upstream skip: …"]`.
// Any other `#[ignore]` is a problem.
//
// Where a test file lives: its §1.6 place under src/, or under the crate's
// tests/ (Cargo integration tests, compiled from tests/*.rs roots) at the same
// path without `src/`, or flat as tests/<file name>. A key found in two places
// is a problem.
//
// Exit status is 1 on any missing case or problem.
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const posix = (p) => p.split(sep).join("/");
const rel = (abs) => posix(relative(root, abs));
const arg = (name) => (process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : undefined);

// mapping-guide §1.6: see the header.
export function testFnName(title) {
  let name = title.normalize("NFKD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "_");
  if (/^[0-9]/.test(name)) name = `t_${name}`;
  return name.slice(0, 80);
}

if (arg("--name") !== undefined) {
  console.log(testFnName(arg("--name")));
  process.exit(0);
}

const os = arg("--os") ?? { darwin: "macos", linux: "linux", win32: "windows" }[process.platform];
const onlyPackage = arg("--package");
const upstream = JSON.parse(readFileSync(join(root, `tests/parity/upstream-${os}.json`), "utf8"));
const scope = JSON.parse(readFileSync(join(root, "docs/migration/scope/files.json"), "utf8"));

const crateDirs = {
  cli: { upstream: "upstream/raft-source/packages/cli/", crate: "crates/raft-cli/" },
  shared: { upstream: "upstream/raft-source/packages/shared/", crate: "crates/raft-shared/" },
  "trace-client": { upstream: "upstream/raft-source/packages/trace-client/", crate: "crates/raft-trace-client/" },
  daemon: { upstream: "upstream/raft-source/packages/daemon/", crate: "crates/raft-daemon-core/" },
  computer: { upstream: "upstream/raft-source/packages/computer/", crate: "crates/raft-computer/" },
  oar: { upstream: "upstream/oar/", crate: "crates/oar/" },
};

const snake = (segment) =>
  segment
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2")
    .replace(/[-.]/g, "_")
    .toLowerCase();

export function rustTestFileFor(pkg, upstreamFile) {
  const dirs = crateDirs[pkg];
  let path = upstreamFile.slice(dirs.upstream.length).replace(/\.test\.m?[jt]sx?$/, "").replace(/\.m?[jt]sx?$/, "");
  if (pkg === "oar") path = path.replace(/^packages\/oar\//, "").replace(/^tests\//, "src/tests/");
  return `${dirs.crate}${path.split("/").map(snake).join("/")}_tests.rs`;
}

const EXTRA_TEST_LOCATIONS = [
  { prefix: "crates/raft-shared/src/js/", reason: "decisions.md D3: golden and unit tests of the JS semantics helpers" },
  { prefix: "crates/raft-shared/src/schema/", reason: "decisions.md D2: zod combinator goldens (tools/zod-golden/combinators.mjs)" },
  { prefix: "crates/raft-shared/src/json_schema/", reason: "decisions.md D15: Ajv and safe-regex2 goldens (tools/golden/json-schema.mjs)" },
  { prefix: "crates/commander/", reason: "decisions.md D1: commander 12.1.0 behaviour goldens (tools/golden/commander.mjs)" },
];
const isExtraLocation = (file) => EXTRA_TEST_LOCATIONS.some((l) => file.startsWith(l.prefix));

// A test file lives at its §1.6 place under src/, or (process tests that need
// env!("CARGO_BIN_EXE_…")) under the crate's tests/ at the same relative path
// without the leading src/, or flat as tests/<file name>.
export function rustTestFilesFor(pkg, upstreamFile) {
  const primary = rustTestFileFor(pkg, upstreamFile);
  const crate = crateDirs[pkg].crate;
  const inner = primary.slice(crate.length).replace(/^src\//, "");
  return [...new Set([primary, `${crate}tests/${inner}`, `${crate}tests/${inner.split("/").at(-1)}`])];
}

const rulesJson = JSON.parse(readFileSync(join(root, "docs/migration/scope/rules.json"), "utf8"));
const skipRules = rulesJson.upstreamSkips?.rules ?? [];
const skipKindFor = (c) => skipRules.find((r) => r.file === c.file && (r.title === "*" || r.title === c.title));

const escapeKey = (s) => s.replace(/\r/g, "\\r").replace(/\n/g, "\\n").replace(/\t/g, "\\t");
export const caseKey = (c) => escapeKey([...c.describe, c.title].join(" > "));

const problems = [];

// ---- Rust side: module tree, test fns, key lines ---------------------------
function listRs(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "target") continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) listRs(full, out);
    else if (entry.name.endsWith(".rs")) out.push(full);
  }
  return out;
}

const stripBlockComments = (text) => text.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));

// Files reachable from the crate roots through `mod x;` declarations. Roots:
// src/lib.rs, src/main.rs, src/bin/*.rs, and each Cargo integration test
// tests/*.rs. Inline `mod x { … }` blocks nest the directory of the `mod y;`
// lines inside them, as rustc does.
function compiledFiles(crateDir) {
  const abs = join(root, crateDir);
  const roots = ["src/lib.rs", "src/main.rs"].map((f) => join(abs, f)).filter(existsSync);
  for (const dir of ["src/bin", "tests"]) {
    if (existsSync(join(abs, dir))) roots.push(...readdirSync(join(abs, dir)).filter((f) => f.endsWith(".rs")).map((f) => join(abs, dir, f)));
  }
  const seen = new Set();
  const queue = roots.map((f) => ({ file: f, modDir: dirname(f) }));
  while (queue.length > 0) {
    const { file, modDir } = queue.shift();
    if (seen.has(file) || !existsSync(file)) continue;
    seen.add(file);
    const lines = stripBlockComments(readFileSync(file, "utf8")).split("\n");
    let pathAttr = null;
    let depth = 0;
    const inline = []; // { name, depth } of open inline modules
    for (const raw of lines) {
      const noComment = raw.replace(/\/\/.*$/, "");
      const line = /#\[\s*path/.test(noComment) ? noComment : noComment.replace(/"(?:[^"\\]|\\.)*"/g, '""');
      const p = line.match(/#\[\s*path\s*=\s*"([^"]+)"\s*\]/);
      if (p) pathAttr = p[1];
      const names = inline.map((i) => i.name);
      const m = line.match(/^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;/);
      const open = line.match(/^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{/);
      if (m) {
        if (pathAttr) {
          const target = resolve(dirname(file), ...names, pathAttr);
          // A `#[path]` file owns its directory, like a mod.rs file.
          queue.push({ file: target, modDir: dirname(target) });
        } else {
          const dir = join(modDir, ...names);
          const a = join(dir, `${m[1]}.rs`);
          const b = join(dir, m[1], "mod.rs");
          queue.push({ file: existsSync(a) ? a : b, modDir: join(dir, m[1]) });
        }
        pathAttr = null;
      } else if (open) {
        inline.push({ name: open[1], depth });
        pathAttr = null;
      } else if (/\S/.test(line) && !/^\s*#\[/.test(line)) {
        pathAttr = null;
      }
      for (const ch of line) {
        if (ch === "{") depth += 1;
        if (ch === "}") {
          depth -= 1;
          while (inline.length > 0 && inline.at(-1).depth >= depth) inline.pop();
        }
      }
    }
  }
  return seen;
}

const TEST_ATTR = /^#\[\s*(?:[A-Za-z_][\w]*::)*(?:test|rstest)\b/;

// Collects each attribute (multi-line safe) before each `fn` in a file.
function scanFns(lines) {
  const fns = [];
  let attrs = [];
  let keyLines = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const t = line.trim();
    const key = line.match(/^\s*\/\/ test: (.*)$/);
    if (key) {
      keyLines.push({ key: key[1], line: i + 1 });
      i += 1;
      continue;
    }
    if (t.startsWith("#[")) {
      let text = t;
      let depth = 0;
      let j = i;
      for (;;) {
        for (const ch of lines[j]) depth += ch === "[" ? 1 : ch === "]" ? -1 : 0;
        if (depth <= 0 || j + 1 >= lines.length) break;
        j += 1;
        text += ` ${lines[j].trim()}`;
      }
      attrs.push(text);
      i = j + 1;
      continue;
    }
    const fn = t.match(/^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)/);
    if (fn) {
      const indent = line.match(/^\s*/)[0].length;
      let end = i;
      while (end + 1 < lines.length && !(lines[end + 1].match(/^\s*/)[0].length <= indent && lines[end + 1].trim().startsWith("}")) && !/\}\s*$/.test(lines[i])) end += 1;
      fns.push({ name: fn[1], line: i + 1, attrs, keys: keyLines, isTest: attrs.some((a) => TEST_ATTR.test(a)), body: lines.slice(i, end + 2).join("\n") });
      attrs = [];
      keyLines = [];
    } else if (t !== "" && !t.startsWith("//")) {
      for (const k of keyLines) fns.push({ orphanKey: k });
      attrs = [];
      keyLines = [];
    }
    i += 1;
  }
  for (const k of keyLines) fns.push({ orphanKey: k });
  return fns;
}

// cfg predicate evaluation for the listing OS.
function evalCfg(expr) {
  const src = expr.trim();
  const call = src.match(/^(not|all|any)\s*\(([\s\S]*)\)$/);
  if (call) {
    const parts = [];
    let depth = 0;
    let cur = "";
    for (const ch of call[2]) {
      if (ch === "(") depth += 1;
      if (ch === ")") depth -= 1;
      if (ch === "," && depth === 0) {
        parts.push(cur);
        cur = "";
      } else cur += ch;
    }
    if (cur.trim()) parts.push(cur);
    const vals = parts.map(evalCfg);
    if (vals.includes(null)) return null;
    return call[1] === "not" ? !vals[0] : call[1] === "all" ? vals.every(Boolean) : vals.some(Boolean);
  }
  if (src === "windows") return os === "windows";
  if (src === "unix") return os !== "windows";
  const tos = src.match(/^target_os\s*=\s*"([^"]+)"$/);
  if (tos) return tos[1] === ({ macos: "macos", linux: "linux", windows: "windows" }[os]);
  return null;
}

function ignoreState(attrs) {
  const out = { bare: false, plain: false, active: false, inactive: false, unknown: [] };
  for (const a of attrs) {
    if (/^#\[\s*ignore\s*=\s*"upstream skip: /.test(a)) out.plain = true;
    else if (/^#\[\s*ignore\b/.test(a)) out.bare = true;
    const m = a.match(/^#\[\s*cfg_attr\s*\(([\s\S]*),\s*ignore\s*=\s*"upstream skip: [^"]*"\s*\)\s*\]$/);
    if (m) {
      const v = evalCfg(m[1]);
      if (v === null) out.unknown.push(m[1]);
      else if (v) out.active = true;
      else out.inactive = true;
    } else if (/^#\[\s*cfg_attr\b[\s\S]*\bignore\b/.test(a)) out.unknown.push(a);
  }
  return out;
}

const RUST_KEYWORDS = new Set("as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self static struct super trait true type unsafe use where while abstract become box do final macro override priv try typeof unsized virtual yield".split(" "));

const rustKeys = new Map(); // rust file → Map(key → { line, fn, attrs })
const crateCounts = {}; // pkg → { rustTests, unkeyed }
for (const [pkg, dirs] of Object.entries(crateDirs)) {
  if (onlyPackage && pkg !== onlyPackage) continue;
  const compiled = compiledFiles(dirs.crate);
  crateCounts[pkg] = { rustTests: 0, extra: 0 };
  for (const abs of listRs(join(root, dirs.crate))) {
    const file = rel(abs);
    const lines = readFileSync(abs, "utf8").split("\n");
    const scanned = scanFns(lines);
    const isCompiled = compiled.has(abs);
    if (!isCompiled) {
      if (file.endsWith("_tests.rs") || scanned.some((f) => f.isTest || f.keys?.length || f.orphanKey)) {
        problems.push(`${file}: not compiled (no \`mod\`/\`#[path] mod\` reaches it from the crate root); its tests do not count`);
      }
      continue;
    }
    const unkeyed = [];
    for (const f of scanned) {
      if (f.orphanKey) {
        problems.push(`${file}:${f.orphanKey.line}: \`// test:\` line not followed by a test function`);
        continue;
      }
      const testAttrs = f.attrs.filter((a) => TEST_ATTR.test(a));
      if (f.isTest && f.keys.length > 0) crateCounts[pkg].rustTests += 1;
      else if (f.isTest && isExtraLocation(file)) crateCounts[pkg].extra += 1;
      if (f.isTest && f.attrs.some((a) => /^#\[\s*ignore\b/.test(a) && !/^#\[\s*ignore\s*=\s*"upstream skip: /.test(a))) problems.push(`${file}:${f.line}: bare #[ignore] on ${f.name} is not allowed`);
      if (f.keys.length === 0) {
        if (f.isTest && !isExtraLocation(file)) unkeyed.push(`${f.name}:${f.line}`);
        continue;
      }
      if (f.keys.length > 1) problems.push(`${file}:${f.keys[1].line}: ${f.keys.length} \`// test:\` lines on one fn ${f.name}`);
      if (testAttrs.length !== 1) problems.push(`${file}:${f.keys[0].line}: fn ${f.name} has ${testAttrs.length} test attributes, needs exactly 1`);
      const keys = rustKeys.get(file) ?? new Map();
      rustKeys.set(file, keys);
      const { key, line } = f.keys[0];
      if (keys.has(key)) problems.push(`${file}:${line}: duplicate key ${JSON.stringify(key)}`);
      keys.set(key, { line, fn: f.name, attrs: f.attrs, body: f.body });
    }
    if (unkeyed.length > 0) problems.push(`${file}: ${unkeyed.length} test fn(s) without a \`// test:\` key outside EXTRA_TEST_LOCATIONS (${unkeyed.slice(0, 3).join(", ")}${unkeyed.length > 3 ? ", …" : ""})`);
  }
}

// ---- Waivers ---------------------------------------------------------------
const CATEGORIES = new Set(["dropped", "source-scan", "packaging", "node-preflight"]);
const waivers = new Map();
const waiverRows = [];
const assertionRows = [];
const waiverPath = join(root, "tests-waived.md");
const unescapeCell = (c) => {
  let v = c.trim();
  if (v.length >= 2 && v.startsWith("`") && v.endsWith("`")) v = v.slice(1, -1);
  return v.replace(/\\\|/g, "|");
};
if (existsSync(waiverPath)) {
  readFileSync(waiverPath, "utf8").split("\n").forEach((line, index) => {
    if (!line.startsWith("|")) return;
    const cells = line.replace(/^\|/, "").replace(/(?<!\\)\|\s*$/, "").split(/(?<!\\)\|/);
    if (cells.length !== 4) {
      problems.push(`tests-waived.md:${index + 1}: table row has ${cells.length} cells, needs 4`);
      return;
    }
    if (cells[0].trim() === "Upstream file" || /^-+$/.test(cells[0].trim())) return;
    const [file, rawKey, category, reason] = cells.map(unescapeCell);
    const split = rawKey.indexOf(" :: assert ");
    const key = split === -1 ? rawKey : rawKey.slice(0, split);
    const assertion = split === -1 ? null : rawKey.slice(split + " :: assert ".length);
    const row = { file, key, assertion, category, reason, line: index + 1, hits: 0 };
    if (!CATEGORIES.has(category)) problems.push(`tests-waived.md:${row.line}: category ${JSON.stringify(category)} is not one of ${[...CATEGORIES].join(", ")} (mapping-guide §12, D9)`);
    const status = scope.tests[file]?.status;
    if (key === "*" && status !== "waive") problems.push(`tests-waived.md:${row.line}: \`*\` row for a file classified ${status ?? "unclassified"}; \`*\` needs status waive in files.json`);
    if (assertion !== null) {
      if (status !== "port" && status !== "port-partial") problems.push(`tests-waived.md:${row.line}: assertion row for a file classified ${status ?? "unclassified"}`);
      if (category !== "dropped") problems.push(`tests-waived.md:${row.line}: assertion rows use category dropped`);
      if (!assertion.trim()) problems.push(`tests-waived.md:${row.line}: assertion row names no assertion`);
      assertionRows.push(row);
      return;
    }
    if (key !== "*" && status !== "port-partial") problems.push(`tests-waived.md:${row.line}: per-case row for a file classified ${status ?? "unclassified"}; per-case rows need status port-partial`);
    if (waivers.has(`${file}\u0000${key}`)) problems.push(`tests-waived.md:${row.line}: duplicate row`);
    waivers.set(`${file}\u0000${key}`, row);
    waiverRows.push(row);
  });
}
const waiverFor = (file, key) => waivers.get(`${file}\u0000${key}`) ?? waivers.get(`${file}\u0000*`);

if (process.argv.includes("--write-file-waivers")) {
  const esc = (s) => s.replace(/\|/g, "\\|");
  const rows = Object.entries(scope.tests)
    .filter(([, t]) => t.status === "waive")
    .map(([file, t]) => {
      const category = /^(packaging|source-scan|node-preflight):/.exec(t.reason)?.[1] ?? "dropped";
      return `| \`${file}\` | * | ${category} | ${esc(t.reason.replace(/^(packaging|source-scan|node-preflight|dropped): /, ""))} |`;
    });
  const manual = [...waiverRows.filter((r) => r.key !== "*"), ...assertionRows]
    .map((r) => `| \`${r.file}\` | \`${esc(r.key)}${r.assertion !== null ? ` :: assert ${esc(r.assertion)}` : ""}\` | ${r.category} | ${esc(r.reason)} |`);
  writeFileSync(waiverPath, [
    "# Waived upstream tests",
    "",
    "Only the categories in mapping-guide.md §12 and decisions.md D9 may appear here:",
    "`dropped` (a dropped feature, named in the reason), `source-scan`, `packaging`,",
    "`node-preflight`. `*` waives every case of a file classified `waive` in",
    "docs/migration/scope/files.json; those rows are generated by",
    "`node tools/test-parity/check.mjs --write-file-waivers`. Per-case rows (key =",
    "`describe > … > title` exactly as the `// test:` line would carry it, `|` written",
    "as `\\|`) are only for files classified `port-partial`. A row whose key is",
    "`<case key> :: assert <what>` does not waive the case: the case is ported and",
    "counts as ported, and the row names one upstream assertion (or injected input)",
    "the Rust test leaves out because it belongs to a dropped feature (category",
    "`dropped`); allowed for files classified `port` or `port-partial`.",
    "Per-case and assertion rows are kept by the generator.",
    "",
    "| Upstream file | Key | Category | Reason |",
    "|---|---|---|---|",
    ...rows.sort(),
    ...manual,
    "",
  ].join("\n"));
  console.log(`wrote ${rows.length} whole-file waivers, kept ${manual.length} per-case rows`);
  process.exit(0);
}

// ---- Compare ---------------------------------------------------------------
const report = {};
const missing = [];
const matched = new Set();
const listedFiles = new Set();
for (const [pkg, cases] of Object.entries(upstream.packages)) {
  if (onlyPackage && pkg !== onlyPackage) continue;
  const row = { upstream: 0, inScope: 0, ported: 0, waived: 0, upstreamSkipped: 0, missing: 0, outOfScope: 0, rustTests: crateCounts[pkg].rustTests, extra: crateCounts[pkg].extra };
  // §1.6 expected fn names, per upstream file in oracle order.
  const expectedNames = new Map();
  const byFile = new Map();
  for (const c of cases) if (!c.error) byFile.set(c.file, [...(byFile.get(c.file) ?? []), c]);
  for (const [file, fileCases] of byFile) {
    const used = new Map();
    const names = new Set();
    for (const c of fileCases) {
      const base = testFnName(c.title);
      const n = (used.get(base) ?? 0) + 1;
      used.set(base, n);
      const name = n === 1 ? base : `${base}_${n}`;
      if (names.has(name)) problems.push(`${file}: §1.6 name ${name} collides for ${JSON.stringify(caseKey(c))}`);
      if (RUST_KEYWORDS.has(name)) problems.push(`${file}: §1.6 name ${name} is a Rust keyword`);
      names.add(name);
      expectedNames.set(`${file}\u0000${caseKey(c)}`, name);
    }
  }
  for (const c of cases) {
    if (c.error) {
      problems.push(`${pkg}: test listing error: ${c.file ? `${c.file}: ` : ""}${c.error}`);
      continue;
    }
    row.upstream += 1;
    listedFiles.add(c.file);
    const status = scope.tests[c.file]?.status;
    if (!status) {
      problems.push(`${c.file}: listed upstream but not classified in docs/migration/scope/files.json`);
      continue;
    }
    const key = caseKey(c);
    const waiver = waiverFor(c.file, key);
    if (waiver) waiver.hits += 1;
    for (const a of assertionRows) if (a.file === c.file && a.key === key) a.hits += 1;
    if (status === "out-of-scope" && !waiver) {
      row.outOfScope += 1;
      continue;
    }
    row.inScope += 1;
    if (c.outcome === "skipped" && !waiver && !skipKindFor(c)) problems.push(`${c.file}: upstream skips ${JSON.stringify(key)} on ${os} but rules.json upstreamSkips gives no kind (platform, opt-in, always)`);
    const candidates = rustTestFilesFor(pkg, c.file);
    const hits = candidates.filter((f) => rustKeys.get(f)?.has(key));
    const rustFile = hits[0] ?? candidates[0];
    const found = hits.length > 0 ? rustKeys.get(rustFile).get(key) : null;
    if (hits.length > 1) problems.push(`${hits.join(" and ")}: key ported twice: ${JSON.stringify(key)}`);
    if (found) {
      for (const h of hits) matched.add(`${h}\u0000${key}`);
      row.ported += 1;
      if (waiver) problems.push(`${rustFile}:${found.line}: ported case is also waived (tests-waived.md:${waiver.line})`);
      const expected = expectedNames.get(`${c.file}\u0000${key}`);
      if (found.fn !== expected) problems.push(`${rustFile}:${found.line}: test fn is ${found.fn}, §1.6 gives ${expected}`);
      const ig = ignoreState(found.attrs);
      for (const u of ig.unknown) problems.push(`${rustFile}:${found.line}: cannot evaluate ignore attribute ${u}`);
      const skip = skipKindFor(c);
      if (c.outcome === "skipped" && skip?.kind !== "opt-in") row.upstreamSkipped += 1;
      if (ig.plain && skip?.kind !== "always") problems.push(`${rustFile}:${found.line}: #[ignore = "upstream skip: …"] is only for an unconditional upstream test.skip (rules.json upstreamSkips kind always)`);
      if (skip?.kind === "always") {
        if (!ig.plain) problems.push(`${rustFile}:${found.line}: upstream always skips this (${skip.source}); needs #[ignore = "upstream skip: …"]`);
      } else if (skip?.kind === "opt-in") {
        if (ig.active) problems.push(`${rustFile}:${found.line}: upstream opt-in skip (${skip.env}) must not be an ignore attribute`);
        if (!found.body.includes(`"${skip.env}"`) && !found.body.includes(`// upstream opt-in: ${skip.env}`)) {
          problems.push(`${rustFile}:${found.line}: upstream skips this unless ${skip.env} is set (${skip.source}); the test must read "${skip.env}" at runtime and return early, or carry \`// upstream opt-in: ${skip.env}\``);
        }
      } else if (c.outcome === "skipped") {
        if (!ig.active) problems.push(`${rustFile}:${found.line}: upstream skips this on ${os}${c.skipReason ? ` (${c.skipReason})` : ""}; needs #[cfg_attr(<cfg>, ignore = "upstream skip: …")] active on ${os}`);
      } else if (ig.active) {
        problems.push(`${rustFile}:${found.line}: upstream runs this on ${os}, but an ignore attribute is active on ${os}`);
      }
    } else if (waiver) {
      row.waived += 1;
    } else {
      row.missing += 1;
      missing.push(`${pkg}\t${c.file}\t${key}\t→ ${rustFile}`);
    }
  }
  if (row.rustTests !== row.ported) problems.push(`${pkg}: Gate 5: ${crateDirs[pkg].crate} has ${row.rustTests} keyed test fns, ported cases are ${row.ported}`);
  report[pkg] = row;
}
for (const [file, keys] of rustKeys) {
  for (const [key, { line }] of keys) {
    if (!matched.has(`${file}\u0000${key}`) && (!onlyPackage || file.startsWith(crateDirs[onlyPackage].crate))) {
      problems.push(`${file}:${line}: key matches no in-scope upstream case: ${JSON.stringify(key)}`);
    }
  }
}
if (!onlyPackage) {
  for (const row of [...waiverRows, ...assertionRows]) {
    if (!listedFiles.has(row.file)) problems.push(`tests-waived.md:${row.line}: stale row, ${row.file} is not in the ${os} listing`);
    else if (row.hits === 0) problems.push(`tests-waived.md:${row.line}: stale row, key matches no listed case: ${JSON.stringify(row.key)}`);
  }
  for (const r of skipRules) if (!listedFiles.has(r.file)) problems.push(`rules.json upstreamSkips: ${r.file} is not in the ${os} listing`);
  for (const [file, t] of Object.entries(scope.tests)) {
    if (t.status === "port-partial" && listedFiles.has(file) && !waiverRows.some((r) => r.file === file && r.key !== "*")) {
      problems.push(`${file}: port-partial but tests-waived.md has no per-case row for it`);
    }
  }
}

console.log(`parity (${os}; upstream listed with ${[...new Set(Object.values(upstream.meta ?? {}).map((m) => m.node))].join(", ") || "unknown node"})`);
console.log("package        upstream  in-scope  ported  waived  up-skip  missing  out-of-scope  rust-tests  extra");
for (const [pkg, r] of Object.entries(report)) {
  console.log(`${pkg.padEnd(14)} ${[r.upstream, r.inScope, r.ported, r.waived, r.upstreamSkipped, r.missing, r.outOfScope, r.rustTests, r.extra].map((n, i) => String(n).padStart([8, 9, 7, 7, 8, 8, 13, 11, 6][i])).join(" ")}`);
}
if (process.argv.includes("--missing")) for (const m of missing) console.log(`missing\t${m}`);
for (const p of problems) console.log(`problem\t${p}`);
process.exitCode = missing.length > 0 || problems.length > 0 ? 1 : 0;

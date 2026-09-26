// Enumerates every upstream test case by running the oracle (decisions.md D9).
// Usage: node tools/test-list/list.mjs <package[,package…]|all> [--os <name>]
// Writes tests/parity/upstream-<os>.json (merged with packages already listed)
// and tests/parity/upstream-<os>.cmd (the commands that produced it).
//
// Prerequisites (the oracle as upstream runs it):
// - `pnpm install` in upstream/raft-source and upstream/oar, Node 24.15.0.
// - oar: tests/cli-progress.test.ts and apps/coxswain import `@botiverse/oar`
//   through its package exports, which point at dist/. This script runs
//   `pnpm --filter @botiverse/oar build` (tsc; dist/ is gitignored upstream)
//   before listing oar, so those files load instead of failing to resolve.
//
// Environment: vitest runs with CI=1, as the Gate 6 GitHub Actions jobs do.
// It is deliberate: upstream configs switch snapshot `update` off under CI,
// and any `skipIf(process.env.CI)` is then evaluated as on CI.
// Every runner gets TZ=Asia/Shanghai: upstream tests assert +08:00 local time
// output (decisions.md D23).
//
// Failure is loud. A package whose runner exits non-zero, whose report lists
// a file that failed to load, or that has any failed case gets `{ error }`
// records next to its cases (tools/test-parity reports them as problems),
// and this script exits 1.
//
// vitest's JSON reporter carries no skip reason, so vitest cases have no
// `skipReason`; node:test cases carry the reason given to `{ skip }`.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const posix = (p) => p.split(sep).join("/");
const rel = (abs) => posix(relative(root, abs));
const os = process.argv.includes("--os") ? process.argv[process.argv.indexOf("--os") + 1] : { darwin: "macos", linux: "linux", win32: "windows" }[process.platform];
const workDir = join(root, "target/test-list", os);
mkdirSync(workDir, { recursive: true });

const packages = {
  cli: { dir: "upstream/raft-source/packages/cli", runner: "node-test", glob: "src" },
  shared: {
    dir: "upstream/raft-source/packages/shared",
    runner: "node-test",
    glob: "src",
    // `pnpm test` runs this script before node --test; it is one pass/fail check.
    scripts: [{ file: "scripts/check-agent-api-message-lockstep.mjs", title: "test:message-lockstep" }],
  },
  "trace-client": { dir: "upstream/raft-source/packages/trace-client", runner: "node-test", glob: "src" },
  computer: { dir: "upstream/raft-source/packages/computer", runner: "vitest" },
  // test:sea-host is listed (not run): running it builds a real SEA.
  daemon: { dir: "upstream/raft-source/packages/daemon", runner: "vitest", listConfigs: ["vitest.sea-host.config.ts"] },
  oar: { dir: "upstream/oar", runner: "vitest", prebuild: ["pnpm", ["--filter", "@botiverse/oar", "build"]] },
};

const commands = [];
function run(cmd, args, options) {
  const shown = [cmd === process.execPath ? "node" : cmd, ...args].map((a) => posix(a).split(posix(root)).join("$REPO"));
  commands.push(`(cd ${posix(relative(root, options.cwd)) || "."} && ${shown.join(" ")})`);
  return spawnSync(cmd, args, { ...options, shell: process.platform === "win32" && cmd !== process.execPath });
}

function testFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules") continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...testFiles(full));
    else if (entry.name.endsWith(".test.ts")) out.push(full);
  }
  return out.sort();
}

const byFile = (a, b) => (a.file < b.file ? -1 : a.file > b.file ? 1 : 0);

function listNodeTest(name, pkg) {
  const cwd = join(root, pkg.dir);
  const dest = join(workDir, `${name}.ndjson`);
  const files = testFiles(join(cwd, pkg.glob)).map((f) => posix(relative(cwd, f)));
  const scriptCases = [];
  for (const script of pkg.scripts ?? []) {
    const r = run(process.execPath, [script.file], { cwd, stdio: ["ignore", "ignore", "inherit"] });
    scriptCases.push({ file: `${pkg.dir}/${script.file}`, describe: [], title: script.title, outcome: r.status === 0 ? "passed" : "failed" });
  }
  const result = run(process.execPath, [
    "--import", "tsx", "--test",
    `--test-reporter=${join(root, "tools/test-list/node-test-reporter.mjs")}`,
    `--test-reporter-destination=${dest}`,
    ...files,
  ], { cwd, stdio: ["ignore", "ignore", "inherit"], env: { ...process.env, NO_COLOR: "1", TZ: "Asia/Shanghai" } });
  const cases = existsSync(dest) ? readFileSync(dest, "utf8").split("\n").filter(Boolean).map((line) => {
    const e = JSON.parse(line);
    if (e.error) return { file: rel(resolve(cwd, e.file)), error: e.error };
    return { file: rel(resolve(cwd, e.file)), describe: e.describe, title: e.title, outcome: e.outcome, ...(e.skipReason ? { skipReason: e.skipReason } : {}) };
  }) : [];
  const out = [...scriptCases, ...cases];
  if (result.status !== 0) out.push({ error: result.status === null ? `runner killed: ${result.signal}` : `runner exited ${result.status}` });
  return out;
}

function vitestReport(name, cwd, args) {
  const dest = join(workDir, `${name}.json`);
  const result = run("npx", ["vitest", ...args, `--outputFile=${dest}`], { cwd, stdio: ["ignore", "ignore", "inherit"], env: { ...process.env, NO_COLOR: "1", CI: "1", TZ: "Asia/Shanghai" } });
  return { result, report: existsSync(dest) ? JSON.parse(readFileSync(dest, "utf8")) : null };
}

function listVitest(name, pkg) {
  const cwd = join(root, pkg.dir);
  const out = [];
  if (pkg.prebuild) {
    const [cmd, args] = pkg.prebuild;
    const r = run(cmd, args, { cwd, stdio: ["ignore", "ignore", "inherit"] });
    if (r.status !== 0) out.push({ error: `prebuild ${cmd} ${args.join(" ")} exited ${r.status}` });
  }
  const { result, report } = vitestReport(name, cwd, ["run", "--reporter=json"]);
  if (!report) return [...out, { error: `vitest wrote no report (exit ${result.status})` }];
  const outcomeOf = { passed: "passed", failed: "failed", skipped: "skipped", pending: "skipped", todo: "todo", disabled: "skipped" };
  const files = [...report.testResults].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  for (const fileResult of files) {
    const file = rel(fileResult.name);
    if (fileResult.status === "failed" && fileResult.assertionResults.length === 0) {
      out.push({ file, error: `file failed to load: ${(fileResult.message ?? "").split("\n")[0]}` });
    }
    for (const a of fileResult.assertionResults) {
      out.push({ file, describe: a.ancestorTitles, title: a.title, outcome: outcomeOf[a.status] ?? a.status });
    }
  }
  if (result.status !== 0 || report.success === false) out.push({ error: `vitest run exited ${result.status}, success=${report.success}, failed suites ${report.numFailedTestSuites}` });
  // Separate configs are listed, not run (vitest list --json).
  for (const config of pkg.listConfigs ?? []) {
    const dest = join(workDir, `${name}.${config}.json`);
    const r = run("npx", ["vitest", "list", "--json", `--config=${config}`], { cwd, stdio: ["ignore", "pipe", "inherit"], env: { ...process.env, NO_COLOR: "1", CI: "1", TZ: "Asia/Shanghai" } });
    writeFileSync(dest, r.stdout ?? "");
    let listed = null;
    try {
      listed = JSON.parse(r.stdout.toString());
    } catch {
      out.push({ error: `vitest list --config=${config} exited ${r.status} without JSON` });
      continue;
    }
    for (const t of listed.sort(byFile)) {
      const parts = t.name.split(" > ");
      out.push({ file: rel(resolve(cwd, t.file)), describe: parts.slice(0, -1), title: parts.at(-1), outcome: "listed" });
    }
  }
  return out;
}

const wanted = process.argv[2] === "all" || !process.argv[2] ? Object.keys(packages) : process.argv[2].split(",");
const outPath = join(root, `tests/parity/upstream-${os}.json`);
mkdirSync(dirname(outPath), { recursive: true });
const merged = existsSync(outPath) ? JSON.parse(readFileSync(outPath, "utf8")) : { os, packages: {} };
delete merged.node;
merged.meta ??= {};
const upstreamCommit = (dir) => spawnSync("git", ["-C", join(root, dir), "rev-parse", "HEAD"], { encoding: "utf8" }).stdout.trim();
let failed = false;
for (const name of wanted) {
  const pkg = packages[name];
  if (!pkg) throw new Error(`unknown package ${name}; known: ${Object.keys(packages).join(", ")}`);
  const started = Date.now();
  commands.length = 0;
  const cases = pkg.runner === "node-test" ? listNodeTest(name, pkg) : listVitest(name, pkg);
  for (const c of cases.filter((c) => c.outcome === "failed")) cases.push({ file: c.file, error: `upstream case failed: ${[...c.describe, c.title].join(" > ")}` });
  merged.packages[name] = cases;
  merged.meta[name] = { node: process.version, upstreamCommit: upstreamCommit(pkg.dir), commands: [...commands] };
  const counts = cases.reduce((acc, c) => ({ ...acc, [c.error ? "error" : c.outcome]: (acc[c.error ? "error" : c.outcome] ?? 0) + 1 }), {});
  console.log(`${name}: ${cases.filter((c) => !c.error).length} cases in ${Math.round((Date.now() - started) / 1000)}s`, JSON.stringify(counts));
  for (const c of cases.filter((c) => c.error)) console.error(`ERROR ${name}: ${c.file ?? ""} ${c.error}`);
  if (cases.some((c) => c.error)) failed = true;
}
merged.packages = Object.fromEntries(Object.keys(packages).filter((n) => merged.packages[n]).map((n) => [n, merged.packages[n]]));
merged.meta = Object.fromEntries(Object.keys(packages).filter((n) => merged.meta[n]).map((n) => [n, merged.meta[n]]));
writeFileSync(outPath, `${JSON.stringify(merged, null, 1)}\n`);
writeFileSync(join(root, `tests/parity/upstream-${os}.cmd`), [
  `# Regenerate tests/parity/upstream-${os}.json (one package at a time is fine):`,
  `node tools/test-list/list.mjs all --os ${os}`,
  "# Per-package commands of the last run, from tools/test-list/list.mjs:",
  ...Object.entries(merged.meta).flatMap(([n, m]) => m.commands.map((c) => `# ${n}: ${c}`)),
  "",
].join("\n"));
if (failed) {
  console.error("test listing has errors; see { error } records above");
  process.exitCode = 1;
}

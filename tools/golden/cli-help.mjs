// Help and parse-error goldens for `raft` and `raft-computer` (decisions.md D1,
// README Gate 3), captured from the oracle with non-TTY stdio (width 80).
// Usage: node tools/golden/cli-help.mjs <raft|computer> <out.json>
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const [which, out] = process.argv.slice(2);
const home = mkdtempSync(join(tmpdir(), "raft-golden-home-"));
const env = {
  PATH: process.env.PATH,
  HOME: home,
  USERPROFILE: home,
  RAFT_HOME: join(home, ".raft"),
  SLOCK_HOME: join(home, ".slock"),
  LANG: "C",
  TZ: "UTC",
  NO_COLOR: "1",
};

function run(argv) {
  const r = spawnSync(join(root, "tools/oracle.sh"), [which, ...argv], { env, encoding: "utf8", timeout: 60_000 });
  const scrub = (text) => text.split(home).join("<HOME>");
  return { argv, stdout: scrub(r.stdout), stderr: scrub(r.stderr), exit: r.status };
}

function subcommands(helpText) {
  const lines = helpText.split("\n");
  const start = lines.findIndex((l) => l.trim() === "Commands:");
  if (start < 0) return [];
  const names = [];
  for (const line of lines.slice(start + 1)) {
    if (!line.startsWith("  ")) break;
    const m = line.match(/^ {2}(\S+)/);
    if (m && m[1] !== "help" && !line.startsWith("   ")) names.push(m[1].split("|")[0]);
  }
  return names;
}

const help = [];
const queue = [[]];
while (queue.length > 0) {
  const path = queue.shift();
  const result = run([...path, "--help"]);
  help.push(result);
  for (const name of subcommands(result.stdout)) queue.push([...path, name]);
}

const parseCases = which === "raft"
  ? [
      [], ["message"], ["help", "message"], ["help", "nosuch"], ["--help", "--bogus"], ["--version"], ["--version", "extra"],
      ["message", "sned"], ["message", "send", "--contnt", "x"], ["message", "send", "--target"], ["--json"], ["nosuchcmd"],
      ["message", "send", "extra", "positional", "--help"], ["task", "claim", "--number"], ["-p"], ["--profile"],
      ["task", "claim", "--target", "#x", "--number", "1", "--profile"], ["agent"], ["help"], ["help", "help"],
    ]
  : [
      [], ["--version"], ["nosuchcmd"], ["upgrade"], ["channel"], ["operation"], ["logs", "--lines"], ["logs", "--lines", "x"],
      ["logs", "--lines", "5.9"], ["logs", "--lines", "1e25"], ["setup", "--no-start", "--help"], ["start", "--bogus"],
      ["help"], ["help", "logs"], ["help", "nosuch"], ["status", "extra"], ["__print-env", "--help"], ["__build-versions"],
    ];
const parse = parseCases.map(run);

writeFileSync(out, `${JSON.stringify({ oracle: which, node: process.version, help, parse }, null, 1)}\n`);
console.log(`${which}: ${help.length} help paths, ${parse.length} parse cases`);

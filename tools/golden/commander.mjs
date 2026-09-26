// Behaviour goldens for crates/commander (decisions.md D1), captured from
// commander 12.1.0 itself (upstream/raft-source's locked copy).
// Usage: node tools/golden/commander.mjs tests/golden/commander/scenarios.json
//
// Programs are declarative specs so the Rust tests can build the same program
// from the same JSON. Spec fields:
//   program: { name?, description?, version?: [str, flags?, desc?],
//              options?: [opt], arguments?: [str | [str, desc]],
//              commands?: [cmd], helpAfter?: str, preAction?: bool }
//   cmd:     same fields as program plus `name` (the `.command()` string),
//            `hidden?: bool`, `action?: bool` (default true for leaf commands)
//   opt:     { flags, description?, default?, parser?, hidden?, mandatory? }
//            parser names: int (parseInt(v, 10)), collect (previous ?? [] then
//            push), trace (`${String(previous)}|${v}`), invalid (throws
//            InvalidArgumentError("Not a number.") unless /^\d+$/, else Number)
// Case fields:
//   program: key into `programs`; argv: user args (node + script prepended);
//   exitOverride?: bool (default true; false = commander's default exit path,
//     observed by stubbing process.exit); widths?: [out, err] help widths;
//   op?: "parse" (default) | "helpInformation" | "outputHelp" |
//     "visibleCommands" | "options"; path?: [names] for ops on a subcommand.
// Recorded: stdout, stderr, the action calls (command path, positional args,
// opts, optsWithGlobals), preAction hook calls, and the outcome (ok, a
// CommanderError's code/exitCode/message, or the process.exit code).
import { createRequire } from "node:module";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const require = createRequire(resolve(root, "upstream/raft-source/packages/cli/package.json"));
const { Command, Option, InvalidArgumentError, CommanderError } = require("commander");
const commanderVersion = JSON.parse(
  readFileSync(resolve(dirname(require.resolve("commander")), "package.json"), "utf8"),
).version;

const parsers = {
  int: (v) => parseInt(v, 10),
  collect: (v, previous) => [...(previous ?? []), v],
  trace: (v, previous) => `${String(previous)}|${v}`,
  invalid: (v) => {
    if (!/^\d+$/.test(v)) throw new InvalidArgumentError("Not a number.");
    return Number(v);
  },
};

const programs = {
  basic: {
    name: "tool",
    description: "A small tool for exercising commander.",
    version: ["1.2.3"],
    options: [
      { flags: "-p, --profile <slug>", description: "Profile slug to use." },
      { flags: "-v, --verbose", description: "Verbose output." },
    ],
    preAction: true,
    commands: [
      {
        name: "send",
        description: "Send a message to a target.",
        arguments: ["<target>", ["[rest...]", "Extra words."]],
        options: [
          { flags: "--content <text>", description: "Message content." },
          { flags: "-n, --count <n>", description: "How many.", parser: "int" },
          { flags: "--tag <tag>", description: "Repeatable tag.", parser: "collect" },
          { flags: "--trace <v>", description: "Parser previous trace.", parser: "trace" },
          { flags: "--limit <n>", description: "Validated number.", parser: "invalid" },
          { flags: "--dry-run", description: "Do not send." },
          { flags: "--mode [mode]", description: "Optional value.", default: "fast" },
          { flags: "--secret <s>", description: "Hidden option.", hidden: true },
          { flags: "-a", description: "Short flag a." },
          { flags: "-b", description: "Short flag b." },
          { flags: "-c <val>", description: "Short with value." },
        ],
        helpAfter: "\nExamples:\n  tool send #general --content hi\n",
      },
      {
        name: "task",
        description: "Task operations",
        commands: [
          {
            name: "claim",
            description: "Claim a task by number.",
            options: [
              { flags: "--target <target>", description: "Channel target." },
              { flags: "--number <n>", description: "Task number.", parser: "int" },
            ],
          },
          { name: "list", description: "List tasks." },
          { name: "secret-op", description: "Hidden subcommand.", hidden: true },
        ],
      },
      { name: "status", description: "Show status." },
    ],
  },
  computer: {
    name: "comp",
    description: "Default exit path program (no exitOverride).",
    version: ["9.9.9"],
    commands: [
      {
        name: "setup",
        description: "Set up the thing with a rather long description that needs wrapping across multiple lines of help output.",
        arguments: [["<serverSlug>", "The server slug."]],
        options: [
          { flags: "--no-start", description: "Do not start after setup." },
          { flags: "--server-url <url>", description: "Server URL." },
        ],
      },
      {
        name: "logs",
        description: "Show logs.",
        arguments: ["[serverSlug]"],
        options: [
          { flags: "--lines <n>", description: "Number of lines.", parser: "invalid" },
          { flags: "--follow", description: "Follow." },
        ],
      },
      { name: "__service", description: "Internal service entry.", hidden: true },
      { name: "status", description: "Status." },
    ],
  },
  versionFlags: {
    name: "vf",
    version: ["Tool: 2.0.0", "-v, --vers", "print it"],
    options: [{ flags: "--required <r>", description: "Mandatory option.", mandatory: true }],
    commands: [{ name: "run", description: "Run it." }],
  },
  rootAction: {
    name: "ra",
    arguments: ["[file]"],
    options: [{ flags: "--flag", description: "A flag." }],
  },
};

function applyCommon(cmd, spec, calls, path) {
  if (spec.description !== undefined) cmd.description(spec.description);
  if (spec.version) cmd.version(...spec.version);
  for (const arg of spec.arguments ?? []) {
    if (Array.isArray(arg)) cmd.argument(arg[0], arg[1]);
    else cmd.argument(arg);
  }
  for (const o of spec.options ?? []) {
    if (o.hidden || o.mandatory) {
      const opt = new Option(o.flags, o.description);
      if (o.hidden) opt.hideHelp();
      if (o.mandatory) opt.makeOptionMandatory();
      if (o.parser) opt.argParser(parsers[o.parser]);
      if (o.default !== undefined) opt.default(o.default);
      cmd.addOption(opt);
    } else if (o.parser) {
      cmd.option(o.flags, o.description, parsers[o.parser], o.default);
    } else if (o.default !== undefined) {
      cmd.option(o.flags, o.description, o.default);
    } else {
      cmd.option(o.flags, o.description);
    }
  }
  if (spec.helpAfter) cmd.addHelpText("after", spec.helpAfter);
  for (const child of spec.commands ?? []) {
    const sub = cmd.command(child.name, child.hidden ? { hidden: true } : undefined);
    applyCommon(sub, child, calls, [...path, sub.name()]);
  }
  const leaf = !(spec.commands ?? []).length;
  if (leaf && spec.action !== false) {
    cmd.action((...args) => {
      const self = args[args.length - 1];
      calls.push({
        kind: "action",
        path,
        args: args.slice(0, -2),
        opts: self.opts(),
        optsWithGlobals: self.optsWithGlobals(),
      });
    });
  }
}

function build(spec, calls) {
  const program = new Command();
  if (spec.name) program.name(spec.name);
  applyCommon(program, spec, calls, []);
  if (spec.preAction) {
    program.hook("preAction", (thisCommand, actionCommand) => {
      calls.push({ kind: "preAction", hookedOn: thisCommand.name(), actionCommand: actionCommand.name(), opts: program.opts() });
    });
  }
  return program;
}

function find(program, path) {
  let cmd = program;
  for (const name of path ?? []) cmd = cmd.commands.find((c) => c.name() === name);
  return cmd;
}

class ExitCalled extends Error {
  constructor(code) {
    super(`process.exit(${code})`);
    this.exitCode = code;
  }
}

function runCase(c) {
  const calls = [];
  let stdout = "";
  let stderr = "";
  const program = build(programs[c.program], calls);
  const output = {
    writeOut: (s) => { stdout += s; },
    writeErr: (s) => { stderr += s; },
  };
  if (c.widths) {
    output.getOutHelpWidth = () => c.widths[0];
    output.getErrHelpWidth = () => c.widths[1];
  } else {
    output.getOutHelpWidth = () => 80;
    output.getErrHelpWidth = () => 80;
  }
  const configure = (cmd) => {
    cmd.configureOutput(output);
    if (c.exitOverride !== false) cmd.exitOverride();
    for (const sub of cmd.commands) configure(sub);
  };
  configure(program);
  const realExit = process.exit;
  const savedExitCode = process.exitCode;
  process.exitCode = c.exitCode;
  process.exit = (code) => { throw new ExitCalled(code); };
  let outcome;
  let result;
  try {
    const op = c.op ?? "parse";
    if (op === "parse") {
      program.parse(["node", "script", ...c.argv]);
      outcome = { kind: "ok" };
    } else if (op === "helpInformation") {
      result = find(program, c.path).helpInformation();
      outcome = { kind: "ok" };
    } else if (op === "outputHelp") {
      find(program, c.path).outputHelp(c.error ? { error: true } : undefined);
      outcome = { kind: "ok" };
    } else if (op === "visibleCommands") {
      const cmd = find(program, c.path);
      result = cmd.createHelp().visibleCommands(cmd).map((x) => x.name());
      outcome = { kind: "ok" };
    } else if (op === "options") {
      result = find(program, c.path).options.map((o) => ({ flags: o.flags, long: o.long ?? null, short: o.short ?? null, attributeName: o.attributeName() }));
      outcome = { kind: "ok" };
    }
  } catch (err) {
    if (err instanceof ExitCalled) outcome = { kind: "exit", exitCode: err.exitCode };
    else if (err instanceof CommanderError) outcome = { kind: "commanderError", code: err.code, exitCode: err.exitCode, message: err.message };
    else outcome = { kind: "thrown", message: String(err) };
  } finally {
    process.exit = realExit;
    process.exitCode = savedExitCode;
  }
  return { ...c, stdout, stderr, calls, outcome, ...(result !== undefined ? { result } : {}) };
}

const cases = [
  // Help output.
  ...[[], ["send"], ["task"], ["task", "claim"], ["status"]].map((path) => ({ program: "basic", argv: [...path, "--help"] })),
  { program: "basic", argv: ["help"] },
  { program: "basic", argv: ["help", "send"] },
  { program: "basic", argv: ["help", "task", "claim"] },
  { program: "basic", argv: ["help", "nosuch"] },
  { program: "basic", argv: ["task", "help", "claim"] },
  { program: "basic", argv: ["-h"] },
  { program: "basic", argv: ["send", "-h", "--bogus"] },
  { program: "basic", argv: ["--help"], widths: [40, 40] },
  { program: "basic", argv: ["send", "--help"], widths: [120, 60] },
  { program: "basic", argv: ["task"], widths: [100, 50] },
  { program: "basic", argv: ["task"] },
  { program: "basic", argv: [] },
  { program: "computer", argv: ["--help"] },
  { program: "computer", argv: ["setup", "--help"] },
  { program: "computer", argv: ["setup", "--help"], widths: [30, 30] },
  { program: "computer", argv: ["logs", "--help"] },
  { program: "computer", argv: ["help", "logs"] },
  { program: "versionFlags", argv: ["--help"] },
  { program: "rootAction", argv: ["--help"] },
  // Version.
  { program: "basic", argv: ["--version"] },
  { program: "basic", argv: ["-V"] },
  { program: "basic", argv: ["--version", "extra"] },
  { program: "computer", argv: ["--version"] },
  { program: "versionFlags", argv: ["--vers"] },
  { program: "versionFlags", argv: ["-v"] },
  // Successful parses.
  { program: "basic", argv: ["send", "#general"] },
  { program: "basic", argv: ["send", "#general", "a", "b", "c"] },
  { program: "basic", argv: ["send", "#general", "--content", "hi there"] },
  { program: "basic", argv: ["send", "#general", "--content=hi=there"] },
  { program: "basic", argv: ["send", "--content", "-x", "#general"] },
  { program: "basic", argv: ["send", "#general", "-n", "12abc"] },
  { program: "basic", argv: ["send", "#general", "-n12"] },
  { program: "basic", argv: ["send", "#general", "--count=0x10"] },
  { program: "basic", argv: ["send", "#general", "--tag", "a", "--tag", "b", "--tag=c"] },
  { program: "basic", argv: ["send", "#general", "--trace", "x", "--trace", "y"] },
  { program: "basic", argv: ["send", "#general", "--limit", "42"] },
  { program: "basic", argv: ["send", "#general", "--dry-run"] },
  { program: "basic", argv: ["send", "#general", "--mode"] },
  { program: "basic", argv: ["send", "#general", "--mode", "slow"] },
  { program: "basic", argv: ["send", "#general", "--mode=slow"] },
  { program: "basic", argv: ["send", "#general", "--secret", "s3"] },
  { program: "basic", argv: ["send", "#general", "-ab"] },
  { program: "basic", argv: ["send", "#general", "-abcval"] },
  { program: "basic", argv: ["send", "#general", "-ac", "val"] },
  { program: "basic", argv: ["send", "#general", "--", "--content", "x"] },
  { program: "basic", argv: ["send", "--", "-weird"] },
  { program: "basic", argv: ["-p", "prof", "send", "#general"] },
  { program: "basic", argv: ["send", "#general", "-p", "prof"] },
  { program: "basic", argv: ["send", "#general", "--profile=prof", "-v"] },
  { program: "basic", argv: ["send", "#general", "--profile", "a", "--profile", "b"] },
  { program: "basic", argv: ["--verbose", "task", "claim", "--target", "#x", "--number", "7"] },
  { program: "basic", argv: ["task", "claim", "--target", "#x", "--number", "1", "--profile", "p"] },
  { program: "basic", argv: ["task", "claim", "extra", "positional"] },
  { program: "basic", argv: ["task", "secret-op"] },
  { program: "basic", argv: ["status", "extra"] },
  { program: "rootAction", argv: [] },
  { program: "rootAction", argv: ["f.txt", "--flag"] },
  { program: "rootAction", argv: ["f.txt", "g.txt"] },
  { program: "computer", argv: ["setup", "my-server", "--no-start"] },
  { program: "computer", argv: ["setup", "my-server"] },
  { program: "computer", argv: ["setup", "my-server", "--server-url", "http://x"] },
  { program: "computer", argv: ["logs", "--lines", "5"] },
  { program: "computer", argv: ["__service"] },
  // Errors with exitOverride.
  { program: "basic", argv: ["sned"] },
  { program: "basic", argv: ["nosuchcmd"] },
  { program: "basic", argv: ["stat"] },
  { program: "basic", argv: ["task", "clam"] },
  { program: "basic", argv: ["task", "zzzzzzzz"] },
  { program: "basic", argv: ["send"] },
  { program: "basic", argv: ["send", "#g", "--contnt", "x"] },
  { program: "basic", argv: ["send", "#g", "--content"] },
  { program: "basic", argv: ["send", "#g", "-n"] },
  { program: "basic", argv: ["send", "#g", "--limit", "abc"] },
  { program: "basic", argv: ["send", "#g", "--dry-run=yes"] },
  { program: "basic", argv: ["send", "#g", "-z"] },
  { program: "basic", argv: ["send", "#g", "--DRY-RUN"] },
  { program: "basic", argv: ["send", "#g", "--secre", "x"] },
  { program: "basic", argv: ["--bogus"] },
  { program: "basic", argv: ["-p"] },
  { program: "basic", argv: ["--profile"] },
  { program: "basic", argv: ["task", "claim", "--number"] },
  { program: "basic", argv: ["task", "claim", "--target", "#x", "--number", "1", "--profile"] },
  { program: "basic", argv: ["task", "claim", "--tgt", "#x"] },
  { program: "basic", argv: ["task", "claim", "--verbos"] },
  { program: "basic", argv: ["secret-o"] },
  { program: "basic", argv: ["task", "secret-o"] },
  { program: "versionFlags", argv: ["run"] },
  { program: "versionFlags", argv: ["run", "--required", "x"] },
  { program: "versionFlags", argv: ["--required", "x", "run"] },
  // Commander's default exit path.
  { program: "computer", argv: [], exitOverride: false },
  { program: "computer", argv: ["--help"], exitOverride: false },
  { program: "computer", argv: ["--version"], exitOverride: false },
  { program: "computer", argv: ["nosuchcmd"], exitOverride: false },
  { program: "computer", argv: ["stauts"], exitOverride: false },
  { program: "computer", argv: ["logs", "--lines"], exitOverride: false },
  { program: "computer", argv: ["logs", "--lines", "x"], exitOverride: false },
  { program: "computer", argv: ["logs", "--lines", "5.9"], exitOverride: false },
  { program: "computer", argv: ["logs", "--lines", "1e25"], exitOverride: false },
  { program: "computer", argv: ["setup"], exitOverride: false },
  { program: "computer", argv: ["setup", "--no-start", "--help"], exitOverride: false },
  { program: "computer", argv: ["start", "--bogus"], exitOverride: false },
  { program: "computer", argv: ["status", "--bogus"], exitOverride: false },
  { program: "computer", argv: ["help"], exitOverride: false },
  { program: "computer", argv: ["help", "nosuch"], exitOverride: false },
  { program: "computer", argv: ["help"], exitOverride: false, exitCode: 3 },
  { program: "computer", argv: ["--help"], exitOverride: false, exitCode: 4 },
  // Introspection.
  { program: "basic", argv: [], op: "helpInformation" },
  { program: "basic", argv: [], op: "helpInformation", path: ["send"] },
  { program: "basic", argv: [], op: "helpInformation", path: ["task", "claim"] },
  { program: "computer", argv: [], op: "helpInformation" },
  { program: "basic", argv: [], op: "outputHelp", path: ["task"] },
  { program: "basic", argv: [], op: "outputHelp", path: ["task"], error: true },
  { program: "basic", argv: [], op: "visibleCommands" },
  { program: "basic", argv: [], op: "visibleCommands", path: ["task"] },
  { program: "computer", argv: [], op: "visibleCommands" },
  { program: "basic", argv: [], op: "options", path: ["send"] },
  { program: "computer", argv: [], op: "options", path: ["setup"] },
];

const out = process.argv[2];
mkdirSync(dirname(out), { recursive: true });
const results = cases.map(runCase);
writeFileSync(out, `${JSON.stringify({ commander: commanderVersion, node: process.version, programs, cases: results }, null, 1)}\n`);
console.log(`commander ${commanderVersion}: ${results.length} cases`);

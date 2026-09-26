// Every regex literal and `new RegExp("…", "…")` with literal arguments in the
// in-scope upstream sources and tests (docs/migration/scope/files.json), with
// Node's verdict on a fixed input set. Feeds raft_shared::js::JsRegex goldens
// (decisions.md D3, mapping-guide.md §6.2).
// Usage: node tools/golden/regex-corpus.mjs <out.json>
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const require = createRequire(join(root, "upstream/raft-source/package.json"));
const ts = require("typescript");
const { sources, tests } = JSON.parse(readFileSync(join(root, "docs/migration/scope/files.json"), "utf8"));

const files = [
  ...Object.entries(sources).filter(([, s]) => s.status === "in-scope" || s.status === "test-support").map(([f]) => f),
  ...Object.entries(tests).filter(([, t]) => t.status === "port" || t.status === "port-partial").map(([f]) => f),
].sort();

const u = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
const inputs = [
  "", "abc", "Hello World", "  padded\ttext\r\n", "line1\nline2\r\nline3\rline4", "a😀b\ud83d", "日本語テキスト",
  "user@example.com", "https://raft.build/x?y=1#z", "#engineering:aaaa1111", "@Cody hi", "2026-09-07T12:00:00.000Z",
  "--number 87", "C:\\Users\\x\\file.txt", "/tmp/a/b.json", "0x1F", "task-123_abc", "`code` **bold** [l](u)",
  "ERR_TLS_CERT_ALTNAME_INVALID fetch failed", "getaddrinfo ENOTFOUND api.raft.build",
];

const seen = new Map();
for (const file of files) {
  const text = readFileSync(join(root, file), "utf8");
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const visit = (node) => {
    let pattern;
    let flags;
    if (node.kind === ts.SyntaxKind.RegularExpressionLiteral) {
      const literal = node.text;
      const slash = literal.lastIndexOf("/");
      pattern = literal.slice(1, slash);
      flags = literal.slice(slash + 1);
    } else if (ts.isNewExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "RegExp" && node.arguments?.length && ts.isStringLiteralLike(node.arguments[0])) {
      pattern = node.arguments[0].text;
      flags = node.arguments[1] && ts.isStringLiteralLike(node.arguments[1]) ? node.arguments[1].text : "";
    }
    if (pattern !== undefined) {
      const key = `/${pattern}/${flags}`;
      const entry = seen.get(key) ?? { pattern: u(pattern), flags, sites: [] };
      const { line } = source.getLineAndCharacterOfPosition(node.getStart());
      entry.sites.push(`${file.replace(/^upstream\//, "")}:${line + 1}`);
      seen.set(key, entry);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
}

// Compact encoding: `inputs` once; per regex, `r[i]` is 0 when input i does
// not match, else [index, ...groups] with groups as code-unit arrays (null
// for unmatched). Replacement and split are recorded only for matching inputs.
const records = [];
for (const entry of seen.values()) {
  const pattern = String.fromCharCode(...entry.pattern);
  let re;
  try {
    re = new RegExp(pattern, entry.flags);
  } catch (err) {
    records.push({ ...entry, error: `${err.name}: ${err.message}` });
    continue;
  }
  const single = entry.flags.replace(/[gy]/g, "");
  const r = inputs.map((input) => {
    const m = new RegExp(pattern, single).exec(input);
    return m ? [m.index, ...[...m].map((g) => (g === undefined ? null : u(g)))] : 0;
  });
  const named = inputs.map((input) => new RegExp(pattern, single).exec(input)?.groups ?? null)
    .map((g) => (g ? Object.fromEntries(Object.entries(g).map(([k, v]) => [k, v === undefined ? null : u(v)])) : 0));
  const hits = inputs.map((_, i) => i).filter((i) => r[i] !== 0);
  records.push({
    ...entry,
    r,
    ...(named.some((g) => g !== 0) ? { named } : {}),
    ...(entry.flags.includes("g") ? { all: hits.map((i) => [...inputs[i].matchAll(re)].map((m) => m.index)) } : {}),
    replace: hits.map((i) => u(inputs[i].replace(re, "<$&|$1|$$>"))),
    split: hits.map((i) => inputs[i].split(new RegExp(pattern, single)).map((x) => (x === undefined ? null : u(x)))),
  });
}
writeFileSync(process.argv[2], `${JSON.stringify({ node: process.version, inputs: inputs.map(u), count: records.length, records })}\n`);
console.log(`${records.length} distinct regexes from ${files.length} files`);

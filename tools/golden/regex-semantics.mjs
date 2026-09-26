// Semantics goldens for raft_shared::js::JsRegex beyond the literal corpus
// (decisions.md D3; regex-reviewer-a/b findings). Run with tsx so upstream
// modules that build regexes at runtime can be loaded:
//   (cd upstream/raft-source/packages/shared && node --import tsx \
//      ../../../../tools/golden/regex-semantics.mjs ../../../../tests/golden/raft-shared/regex-semantics.json)
//
// Sections:
//   fold       Case-insensitive single-character matching. BMP code points are
//              grouped by union-find over toUpperCase/toLowerCase links; for
//              every group of size > 1 and every member X, the members Y that
//              /^X$/i (and /^X$/iu) match. Code points outside every group
//              match only themselves under both flags (spot-checked by the
//              `classes` section).
//   classes    For a few class patterns under i / iu, which code points of
//              CLASS_PROBE (all grouped code points plus ASCII) match.
//   dynamic    Every RegExp constructed while importing raftRefs.ts and calling
//              its regex factories (RegExp is wrapped to record them), plus the
//              _format.ts escapeRegExp and osSupervisor.ts tag shapes, run on
//              DYNAMIC_INPUTS: exec with groups, matchAll indices, replace.
//   cases      Targeted API cases: named groups, y, d, replace templates,
//              split limits, lastIndex after test/exec loops.
//   errors     new RegExp(p, f) for invalid patterns: the thrown text, or ok.
import { writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const u = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
const hex = (n) => n.toString(16).padStart(4, "0");

// ---- fold -----------------------------------------------------------------
const parent = new Map();
const find = (x) => {
  while (parent.has(x) && parent.get(x) !== x) x = parent.get(x);
  return x;
};
const union = (a, b) => {
  const ra = find(a);
  const rb = find(b);
  if (!parent.has(ra)) parent.set(ra, ra);
  if (!parent.has(rb)) parent.set(rb, rb);
  if (ra !== rb) parent.set(ra, rb);
};
for (let x = 0; x <= 0xffff; x++) {
  if (x >= 0xd800 && x <= 0xdfff) continue;
  const ch = String.fromCharCode(x);
  for (const t of [ch.toUpperCase(), ch.toLowerCase()]) {
    if (t.length === 1 && t.charCodeAt(0) !== x) union(x, t.charCodeAt(0));
  }
}
const groups = new Map();
for (const x of parent.keys()) {
  const r = find(x);
  if (!groups.has(r)) groups.set(r, []);
  groups.get(r).push(x);
}
const fold = [];
for (const members of groups.values()) {
  if (members.length < 2) continue;
  members.sort((a, b) => a - b);
  for (const x of members) {
    const re = new RegExp(`^\\u${hex(x)}$`, "i");
    const reU = new RegExp(`^\\u${hex(x)}$`, "iu");
    const chars = members.map((y) => String.fromCharCode(y));
    fold.push([x, members.filter((_, i) => re.test(chars[i])), members.filter((_, i) => reU.test(chars[i]))]);
  }
}

// ---- classes --------------------------------------------------------------
const probe = new Set();
for (let x = 0; x < 0x80; x++) probe.add(x);
for (const x of parent.keys()) probe.add(x);
const CLASS_PROBE = [...probe].sort((a, b) => a - b);
const classPatterns = ["[a-z]", "[^a-z]", "\\w", "\\W", "[\\w]", "[^\\w]", "[\\W]", "[^\\W]", "[A-Z0-9_:-]", "\\p{Lu}", "\\P{Lu}", "[k-s]", "s", "k", "i", "ss", "\\u212a"];
const classes = [];
for (const p of classPatterns) {
  for (const flags of ["i", "iu", "u", ""]) {
    let re;
    try {
      re = new RegExp(`^${p}$`, flags);
    } catch {
      continue;
    }
    classes.push({ pattern: p, flags, matches: CLASS_PROBE.filter((x) => re.test(String.fromCharCode(x))) });
  }
}

// ---- dynamic --------------------------------------------------------------
const constructed = [];
const NativeRegExp = globalThis.RegExp;
globalThis.RegExp = new Proxy(NativeRegExp, {
  construct(target, args) {
    const re = new target(...args);
    constructed.push({ source: re.source, flags: re.flags });
    return re;
  },
});
const refs = await import(pathToFileURL(resolve(root, "upstream/raft-source/packages/shared/src/raftRefs.ts")).href);
for (const [name, fn] of Object.entries(refs)) {
  if (typeof fn === "function" && /^create.*Regex$/.test(name)) {
    for (const arg of ["alice", "Bob.Smith", "张三", "a-b_c"]) fn(arg);
  }
}
globalThis.RegExp = NativeRegExp;
const escapeRegExp = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
for (const term of ["deploy", "a.b", "C++", "(x)", "ſ", "straße"]) constructed.push({ source: escapeRegExp(term), flags: "i" });
for (const tag of ["string", "key", "integer"]) constructed.push({ source: `<${tag}>([^<]*)</${tag}>`, flags: "gi" });
// computer/src/output.ts inferNextCommands: one backtick-fence regex per width.
for (const width of [1, 2, 3]) {
  const fence = "`".repeat(width);
  constructed.push({ source: new RegExp("(?<!`)" + fence + "(?!`)([\\s\\S]*?)(?<!`)" + fence + "(?!`)", "g").source, flags: "g" });
}

const DYNAMIC_INPUTS = [
  "hi @alice and @Bob.Smith, see #engineering:abc123 and #general",
  "dm:@carol:deadbeef msg=Abc-123 task #42 and Task  #7",
  "email me at x@alice.com or @张三! (@a-b_c)",
  "#Général:ABCDEF12 dm:@Dave #1 #0 task #9x",
  "computer:1234abcd-12ab-4cde-8f00-123456789abc app:foo.bar",
  "<string>v1</string><STRING>v2</String><key>k</key>",
  "Deploy the fix; DEPLOY again; ſtraße STRASSE a.b C++ (x)",
  "`@alice` ```#general``` @alice",
  "Run `raft-computer start` or ``raft-computer setup `x` now`` then ```raft-computer\nstop```.",
  "````a```b```` `` ` `unterminated",
];
const seen = new Set();
const dynamic = [];
for (const { source, flags } of constructed) {
  const key = `/${source}/${flags}`;
  if (seen.has(key)) continue;
  seen.add(key);
  const single = flags.replace(/[gy]/g, "");
  dynamic.push({
    source: u(source),
    flags,
    exec: DYNAMIC_INPUTS.map((s) => {
      const m = new RegExp(source, single).exec(s);
      return m ? { index: m.index, groups: [...m].map((g) => (g === undefined ? null : u(g))) } : null;
    }),
    all: flags.includes("g") ? DYNAMIC_INPUTS.map((s) => [...s.matchAll(new RegExp(source, flags))].map((m) => m.index)) : null,
    replace: DYNAMIC_INPUTS.map((s) => u(s.replace(new RegExp(source, flags), "[$&|$1|$`]"))),
  });
}

// ---- cases ----------------------------------------------------------------
const cases = [];
const record = (label, fn) => {
  try {
    cases.push({ label, result: fn() });
  } catch (e) {
    cases.push({ label, error: `${e.name}: ${e.message}` });
  }
};
const execView = (m) => (m ? { index: m.index, groups: [...m].map((g) => (g === undefined ? null : g)), named: m.groups ? { ...m.groups } : null, indices: m.indices ? { ranges: [...m.indices], named: m.indices.groups ? { ...m.indices.groups } : null } : null } : null);
record("named groups", () => execView(/(?<year>\d{4})-(?<month>\d{2})(?:-(?<day>\d{2}))?/.exec("on 2026-09 ok")));
record("named groups replace", () => "2026-09-26".replace(/(?<y>\d+)-(?<m>\d+)-(?<d>\d+)/, "$<d>/$<m>/$<y> $<nope>|$<y"));
record("d flag", () => execView(/(a)(?<n>b)?(c)/d.exec("xxac")));
record("d flag unicode", () => execView(/(😀)(?<n>.)/du.exec("a😀b")));
record("sticky test loop", () => {
  const re = /a/y;
  const out = [];
  for (const s of ["aab", "aab", "aab", "baa"]) out.push([re.test(s), re.lastIndex]);
  return out;
});
record("sticky exec positions", () => {
  const re = /\d+/y;
  const s = "12ab345";
  const out = [];
  for (const i of [0, 1, 2, 4, 7, 8]) {
    re.lastIndex = i;
    const m = re.exec(s);
    out.push([i, m ? m[0] : null, re.lastIndex]);
  }
  return out;
});
record("sticky replace", () => "aaba".replace(/a/gy, "x"));
record("sticky split", () => "a,b,,c".split(/,/y));
record("global test loop", () => {
  const re = /o/g;
  const out = [];
  while (re.test("foo boo")) out.push(re.lastIndex);
  out.push(re.lastIndex);
  return out;
});
record("global exec empty unicode", () => {
  const re = /(?:)/gu;
  const out = [];
  let m;
  while ((m = re.exec("a😀b")) && out.length < 10) {
    out.push(m.index);
    if (m.index === re.lastIndex) re.lastIndex++;
  }
  return out;
});
record("lastIndex after failed global exec", () => {
  const re = /x/g;
  re.lastIndex = 3;
  re.exec("abc");
  return re.lastIndex;
});
record("lastIndex fractional and negative", () => {
  const re = /b/g;
  const out = [];
  for (const li of [1.7, -1, 1e10, NaN, "1"]) {
    re.lastIndex = li;
    const m = re.exec("abab");
    out.push([String(li), m ? m.index : null, re.lastIndex]);
  }
  return out;
});
for (const t of ["$$", "$&", "$`", "$'", "$0", "$1", "$01", "$10", "$2", "$<n>", "$", "a$", "$$1", "$11", "$100"]) {
  record(`replace template ${t}`, () => "xaby".replace(/(a)(?<n>b)/, t));
  record(`replace template global ${t}`, () => "abab".replace(/(a)(b)/g, t));
}
record("replaceAll string special", () => "a.a.a".replaceAll(".", "$&$&"));
for (const [p, f, s, limit] of [["", "", "abc", undefined], ["", "u", "a😀b", undefined], ["", "", "a😀b", undefined], [",", "", "a,b,c", 2], [",", "", "a,b,c", 0], [",", "", "a,b,c", -1], [",", "", "a,b,c", 4294967297], ["(,)", "", "a,b", undefined], ["(,)|(;)", "", "a,b;c", undefined], ["x*", "", "axbxxc", undefined], ["\\b", "", "hi there", undefined], ["", "", "", undefined], ["a", "", "", undefined]]) {
  record(`split /${p}/${f} ${JSON.stringify(s)} ${limit}`, () => s.split(new RegExp(p, f), limit));
}
record("match non-global", () => execView("a1b2".match(/\d/)));
record("match global", () => "a1b22".match(/\d+/g));
record("match global none", () => "ab".match(/\d/g));
record("search", () => ["a1b2".search(/\d/), "ab".search(/\d/), "a😀b".search(/b/u)]);
record("matchAll keeps lastIndex", () => {
  const re = /a/g;
  re.lastIndex = 2;
  const idx = [..."aaaa".matchAll(re)].map((m) => m.index);
  return [idx, re.lastIndex];
});
record("\\B inside surrogate pair (u)", () => execView(/\B/u.exec("a😀")));
record("\\B inside surrogate pair", () => execView(/\B/.exec("a😀")));
record("\\u{41} without u", () => [/\u{41}/.test("A"), /\u{2}/.test("uu"), /^\u{3}$/.test("uuu")]);
record("\\u{41} with u", () => [/\u{41}/u.test("A")]);
record("lone \\u without u", () => [/\u/.test("u"), /\u12/.test("u12"), /\x/.test("x"), /\xZ/.test("xZ")]);
record("backref under i", () => [/(a)\1/i.test("aA"), /(ſ)\1/i.test("ſs"), /(k)\1/iu.test("kK")]);
record("source escaping", () => [new RegExp("a\\\nb").source, new RegExp(" ").source, new RegExp("/").source, new RegExp("[/]").source, new RegExp("\\/").source, new RegExp("\n\r").source]);
record("deep nesting", () => [new RegExp("(".repeat(300) + "a" + ")".repeat(300)).test("a"), new RegExp("(?:".repeat(1000) + "a" + ")".repeat(1000)).test("a")]);
record("astral group name", () => execView(new RegExp("(?<\u{1d49c}>x)").exec("x")));
record("\\p RGI_Emoji v", () => execView(/^\p{RGI_Emoji}$/v.exec("👍🏽")));

// ---- errors ---------------------------------------------------------------
const ERROR_PATTERNS = [
  "(", ")", "[", "*", "+", "?", "a**", "a{2}{3}", "{1}", "a{3,1}", "(?<=a)*", "(?=a)*", "\\b+", "\\B*", "^*", "$+", "\\", "a\\", "(?<a>x)(?<a>y)", "(?<1a>x)", "\\k<nope>(?<a>x)",
  "\\k", "\\c", "\\01", "[\\1]", "(?", "(?x)", "(?i:a)", "(?-i:a)", "(?i-i:a)", "[b-a]", "\\p{Nope}", "\\p{L", "\\u{110000}", "\\u{41}+", "\\uD83D\\u12", "[\\d-z]", "x{2,1}", "]", "}", "{", "a{", "a{1", "a{1,", "(?<𝒜>x)", "[[a]]", "[a&&b]", "[a--b]", "\\q{abc}",
];
const errors = [];
for (const p of ERROR_PATTERNS) {
  for (const f of ["", "u", "v"]) {
    try {
      new RegExp(p, f);
      errors.push({ pattern: u(p), flags: f, ok: true });
    } catch (e) {
      errors.push({ pattern: u(p), flags: f, error: `${e.name}: ${e.message}` });
    }
  }
}

writeFileSync(process.argv[2], `${JSON.stringify({ node: process.version, icu: process.versions.icu, unicode: process.versions.unicode, CLASS_PROBE, fold, classes, DYNAMIC_INPUTS: DYNAMIC_INPUTS.map(u), dynamic, cases, errors })}\n`);
console.log(`fold ${fold.length}, classes ${classes.length}, dynamic ${dynamic.length}, cases ${cases.length}, errors ${errors.length}`);

// Combinator-level goldens for raft_shared::schema (decisions.md D2), captured
// from zod itself (upstream/raft-source/packages/shared's locked copy, 4.3.6).
// Usage: node tools/zod-golden/combinators.mjs tests/golden/raft-shared/zod-combinators.json
//
// Schemas are declarative specs so the Rust tests build the same schema from
// the same JSON. Spec: { t: <kind>, ...fields, checks?: [[name, ...args]],
// wrap?: [[wrapper, ...args]] } where wrappers apply in order after checks:
//   optional, nullable, nullish, default <value>, meta <object>, describe <s>,
//   transformIdentity, refine <name>, superRefine <name>.
// Kinds: string, uuid (z.uuid()), url (z.url()), isoDatetime (z.iso.datetime(opts?)),
//   number, int (z.int()), coerceNumber, boolean, literal {value}, enum {values},
//   array {of}, record {key, value}, union {of}, discriminatedUnion {key, of},
//   object {shape, mode: strip|passthrough|strict}, strictObject {shape},
//   extend {base, shape} (base object spec .extend(shape)), undefined, null, unknown.
// Named refinements (the Rust tests implement the same closures):
//   noWhitespace   refine(v => !/\s/.test(v), { message: "A single reaction emoji is required" })
//   atHandle       refine(v => v.startsWith("@") && v.slice(1).trim().length > 0, { message: "assignee must be an @handle" })
//   titleOrDesc    refine(v => v.title !== undefined || v.description !== undefined, { message: "At least one of title or description is required" })
//   exactlyOne     superRefine: Number(v.channel !== undefined) + Number(v.mine === "true") !== 1 →
//                  addIssue({ code: z.ZodIssueCode.custom, message: "exactly one of channel or mine=true is required" })
//   pathIssue      superRefine: if v.a === "bad" → addIssue({ code: "custom", path: ["a"], message: "a is bad" });
//                  and if v.b === "bad" → addIssue({ code: "custom", path: ["b", 0], message: "b is bad" })
// Inputs are JSON with markers: {"$undefined": true} → undefined,
// {"$number": "NaN" | "Infinity" | "-Infinity" | "-0"} → that number.
// Recorded per case: success, data (same markers for undefined/non-finite),
// issues (key order preserved, as JSON.stringify gives it) and error.message.
import { createRequire } from "node:module";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const require = createRequire(resolve(root, "upstream/raft-source/packages/shared/package.json"));
const { z } = require("zod");
const zodVersion = JSON.parse(readFileSync(resolve(dirname(require.resolve("zod")), "package.json"), "utf8")).version;

const refinements = {
  noWhitespace: (s) => s.refine((v) => !/\s/.test(v), { message: "A single reaction emoji is required" }),
  atHandle: (s) => s.refine((v) => v.startsWith("@") && v.slice(1).trim().length > 0, { message: "assignee must be an @handle" }),
  titleOrDesc: (s) => s.refine((v) => v.title !== undefined || v.description !== undefined, { message: "At least one of title or description is required" }),
  exactlyOne: (s) => s.superRefine((v, ctx) => {
    const selectors = Number(v.channel !== undefined) + Number(v.mine === "true");
    if (selectors !== 1) ctx.addIssue({ code: z.ZodIssueCode.custom, message: "exactly one of channel or mine=true is required" });
  }),
  pathIssue: (s) => s.superRefine((v, ctx) => {
    if (v.a === "bad") ctx.addIssue({ code: "custom", path: ["a"], message: "a is bad" });
    if (v.b === "bad") ctx.addIssue({ code: "custom", path: ["b", 0], message: "b is bad" });
  }),
};

function decode(v) {
  if (Array.isArray(v)) return v.map(decode);
  if (v && typeof v === "object") {
    if (v.$undefined === true) return undefined;
    if (typeof v.$number === "string") return v.$number === "-0" ? -0 : Number(v.$number);
    const out = {};
    for (const [k, x] of Object.entries(v)) out[k] = decode(x);
    return out;
  }
  return v;
}

function encode(v) {
  if (v === undefined) return { $undefined: true };
  if (typeof v === "number" && (!Number.isFinite(v) || Object.is(v, -0))) return { $number: Object.is(v, -0) ? "-0" : String(v) };
  if (Array.isArray(v)) return v.map(encode);
  if (v && typeof v === "object") {
    const out = {};
    for (const [k, x] of Object.entries(v)) out[k] = encode(x);
    return out;
  }
  return v;
}

function applyChecks(s, checks) {
  for (const [name, ...args] of checks ?? []) {
    if (name === "regex") s = s.regex(new RegExp(args[0], args[1] ?? ""));
    else s = s[name](...args);
  }
  return s;
}

function build(spec) {
  let s;
  switch (spec.t) {
    case "string": s = z.string(); break;
    case "uuid": s = z.uuid(); break;
    case "url": s = z.url(); break;
    case "isoDatetime": s = spec.opts ? z.iso.datetime(spec.opts) : z.iso.datetime(); break;
    case "number": s = z.number(); break;
    case "int": s = z.int(); break;
    case "coerceNumber": s = z.coerce.number(); break;
    case "boolean": s = z.boolean(); break;
    case "literal": s = z.literal(spec.value); break;
    case "enum": s = z.enum(spec.values); break;
    case "array": s = z.array(build(spec.of)); break;
    case "record": s = z.record(build(spec.key), build(spec.value)); break;
    case "union": s = z.union(spec.of.map(build)); break;
    case "discriminatedUnion": s = z.discriminatedUnion(spec.key, spec.of.map(build)); break;
    case "object": {
      s = z.object(Object.fromEntries(Object.entries(spec.shape).map(([k, v]) => [k, build(v)])));
      if (spec.mode === "passthrough") s = s.passthrough();
      if (spec.mode === "strict") s = s.strict();
      break;
    }
    case "strictObject": s = z.strictObject(Object.fromEntries(Object.entries(spec.shape).map(([k, v]) => [k, build(v)]))); break;
    case "extend": s = build(spec.base).extend(Object.fromEntries(Object.entries(spec.shape).map(([k, v]) => [k, build(v)]))); break;
    case "undefined": s = z.undefined(); break;
    case "null": s = z.null(); break;
    case "unknown": s = z.unknown(); break;
    default: throw new Error(`unknown kind ${spec.t}`);
  }
  s = applyChecks(s, spec.checks);
  for (const [w, ...args] of spec.wrap ?? []) {
    if (w === "refine" || w === "superRefine") s = refinements[args[0]](s);
    else if (w === "transformIdentity") s = s.transform((v) => v);
    else s = s[w](...args);
  }
  return s;
}

const str = (checks, wrap) => ({ t: "string", ...(checks ? { checks } : {}), ...(wrap ? { wrap } : {}) });
const U = { $undefined: true };
const N = (n) => ({ $number: n });

// [name, schema spec, inputs]
const groups = [
  ["string", str(), ["x", "", 1, null, U, true, [], {}, N("NaN")]],
  ["string.trim.min1", str([["trim"], ["min", 1]]), ["  hi  ", "   ", "", "\t\n x  ", "\u0085x\u0085", 5]],
  ["string.trim.min1.max16", str([["trim"], ["min", 1], ["max", 16]]), ["😀", "a".repeat(16), "a".repeat(17), " " + "a".repeat(16) + " ", "😀".repeat(8), "😀".repeat(9)]],
  ["string.min3.custom", str([["min", 3, { message: "too short!" }]]), ["ab", "abc"]],
  ["string.max2", str([["max", 2]]), ["abc", "ab"]],
  ["string.regex.hex64", str([["regex", "^[0-9a-f]{64}$"]]), ["a".repeat(64), "A".repeat(64), "xyz"]],
  ["string.regex.slug", str([["trim"], ["regex", "^[a-z0-9][a-z0-9_.-]{0,62}$"]]), ["ok-slug", " ok ", "-bad", "Bad"]],
  ["string.startsWith", str([["startsWith", "sk_"]]), ["sk_123", "pk_123"]],
  ["string.endsWith", str([["endsWith", ".md"]]), ["a.md", "a.txt"]],
  ["string.uuid", str([["uuid"]]), ["123e4567-e89b-12d3-a456-426614174000", "00000000-0000-0000-0000-000000000000", "ffffffff-ffff-ffff-ffff-ffffffffffff", "123e4567-e89b-02d3-a456-426614174000", "123E4567-E89B-12D3-A456-426614174000", "not-a-uuid", ""]],
  ["z.uuid", { t: "uuid" }, ["123e4567-e89b-12d3-a456-426614174000", "123e4567-e89b-12d3-c456-426614174000", "x", 5]],
  ["string.datetime", str([["datetime"]]), ["2026-04-21T07:00:00Z", "2026-04-21T07:00:00.123Z", "2026-04-21T07:00:00.123456789Z", "2026-04-21T07:00Z", "2026-04-21T07:00:00+08:00", "2026-04-21 07:00:00Z", "2026-02-30T07:00:00Z", "2026-04-21", "x"]],
  ["string.datetime.offset", str([["datetime", { offset: true }]]), ["2026-04-21T07:00:00+08:00", "2026-04-21T07:00:00+0800", "2026-04-21T07:00:00Z", "2026-04-21T07:00:00+08"]],
  ["z.iso.datetime", { t: "isoDatetime" }, ["2026-04-21T07:00:00Z", "2026-04-21T07:00:00+08:00", "bad"]],
  ["string.url", str([["url"]]), ["https://raft.build/x", "mailto:a@b.c", "not a url", "http://", "//x"]],
  ["z.url", { t: "url" }, ["https://raft.build/x", "ftp://x.y", "raft.build", "http://localhost:3000"]],
  ["string.trim.toLowerCase", str([["trim"], ["toLowerCase"]]), ["  AbC  "]],
  ["number", { t: "number" }, [1, 1.5, -0, "1", null, U, N("NaN"), N("Infinity"), true]],
  ["number.int.positive", { t: "number", checks: [["int"], ["positive"]] }, [1, 0, -1, 1.5, 9007199254740993, "3"]],
  ["number.int.nonnegative", { t: "number", checks: [["int"], ["nonnegative"]] }, [0, -1, 2]],
  ["number.finite", { t: "number", checks: [["finite"]] }, [1, N("Infinity")]],
  ["number.safe", { t: "number", checks: [["safe"]] }, [9007199254740991, 9007199254740992, -9007199254740992, 1.5]],
  ["number.min.max", { t: "number", checks: [["min", 1], ["max", 100]] }, [0, 1, 100, 101]],
  ["number.int.min.max", { t: "number", checks: [["int"], ["min", 1], ["max", 500]] }, [0, 501, 5, 2.5]],
  ["z.int", { t: "int" }, [1, 1.5, 2 ** 53, "1"]],
  ["z.int.positive", { t: "int", checks: [["positive"]] }, [0, 5]],
  ["coerce.number.int.positive", { t: "coerceNumber", checks: [["int"], ["positive"]] }, ["16", "0x10", " 12 ", "", "abc", "Infinity", "1e3", "1.5", "-3", true, null, U, [], ["7"], {}]],
  ["boolean", { t: "boolean" }, [true, false, "true", 0, null]],
  ["literal.str", { t: "literal", value: "true" }, ["true", "false", true]],
  ["literal.num", { t: "literal", value: 1 }, [1, 2, "1"]],
  ["literal.bool", { t: "literal", value: false }, [false, true]],
  ["enum", { t: "enum", values: ["public", "private", "dm"] }, ["public", "dm", "other", 1, null]],
  ["array.string", { t: "array", of: str([["trim"], ["min", 1]]) }, [[], [" a ", "b"], ["a", "", 3], "a", null, { "0": "a" }]],
  ["array.min1.max2", { t: "array", of: { t: "number" }, checks: [["min", 1], ["max", 2]] }, [[], [1], [1, 2, 3]]],
  ["array.nonempty", { t: "array", of: { t: "string" }, checks: [["nonempty"]] }, [[], ["x"]]],
  ["record.string.unknown", { t: "record", key: { t: "string" }, value: { t: "unknown" } }, [{}, { b: 1, a: [2], "2": 3, "1": 0 }, [], null, "x"]],
  ["record.string.string", { t: "record", key: { t: "string" }, value: str([["trim"]]) }, [{ a: " x ", b: 3 }, { z: "y" }]],
  ["record.enumKey", { t: "record", key: { t: "enum", values: ["a", "b"] }, value: { t: "number" } }, [{ a: 1, b: 2 }, { a: 1 }, { a: 1, b: 2, c: 3 }]],
  ["union", { t: "union", of: [str([["trim"], ["min", 1]]), { t: "number", checks: [["int"]] }] }, [" x ", 3, 1.5, "", null, true]],
  ["union.objects", { t: "union", of: [{ t: "object", shape: { a: { t: "string" } } }, { t: "object", shape: { b: { t: "number" } } }] }, [{ a: "x" }, { b: 1 }, { c: 1 }, { a: 1 }]],
  [
    "discriminatedUnion",
    {
      t: "discriminatedUnion",
      key: "type",
      of: [
        { t: "object", shape: { type: { t: "literal", value: "text" }, text: str([["trim"], ["min", 1]]) } },
        { t: "object", shape: { type: { t: "literal", value: "link" }, url: { t: "url" }, label: str([["trim"]], [["optional"]]) } },
        { t: "object", shape: { type: { t: "enum", values: ["a", "b"] }, n: { t: "number" } } },
      ],
    },
    [{ type: "text", text: " hi ", extra: 1 }, { type: "link", url: "https://x.y" }, { type: "link", url: "nope" }, { type: "other" }, {}, { type: 5 }, "text", null, { type: "a", n: 1 }, { type: "b" }],
  ],
  [
    "object.strip",
    { t: "object", shape: { b: str([["trim"]]), a: { t: "number" }, d: str(null, [["default", "x"]]) } },
    [{ z: 1, a: 2, "5": 0, b: " hi " }, { a: 1 }, { b: "x", a: "no", d: 3 }, null, [], "s", { b: "x", a: 1, d: U }],
  ],
  [
    "object.passthrough",
    { t: "object", mode: "passthrough", shape: { b: str([["trim"]]), a: { t: "number" }, d: str(null, [["default", "x"]]) } },
    [{ z: 1, a: 2, "5": 0, b: " hi " }, { a: 2, b: "q", "10": 1, "2": 2, y: U }, { a: "bad", b: 1, extra: true }],
  ],
  [
    "object.strict",
    { t: "object", mode: "strict", shape: { a: { t: "number" }, b: str(null, [["optional"]]) } },
    [{ a: 1 }, { a: 1, c: 2, d: 3 }, { a: "x", c: 2 }, { b: "x" }],
  ],
  ["strictObject", { t: "strictObject", shape: { a: { t: "number" } } }, [{ a: 1 }, { a: 1, x: 1 }]],
  [
    "object.optional.nullable",
    {
      t: "object",
      shape: {
        o: str(null, [["optional"]]),
        n: str(null, [["nullable"]]),
        nn: str(null, [["nullish"]]),
        on: str([["trim"]], [["nullable"], ["optional"]]),
      },
    },
    [{}, { o: U, n: null, nn: null, on: " x " }, { n: "x" }, { o: null, n: U }, { n: null, nn: U, on: null }],
  ],
  [
    "object.defaults",
    {
      t: "object",
      shape: {
        visibility: { t: "enum", values: ["public", "private"], wrap: [["default", "public"]] },
        scopes: { t: "array", of: { t: "string" }, wrap: [["default", []]] },
        set: { t: "record", key: { t: "string" }, value: { t: "unknown" }, wrap: [["default", {}]] },
      },
    },
    [{}, { visibility: "private", scopes: ["a"] }, { visibility: "bogus" }, { scopes: U }],
  ],
  [
    "object.nested",
    {
      t: "object",
      mode: "passthrough",
      shape: {
        channel: { t: "object", shape: { id: { t: "uuid" }, name: str([["trim"], ["min", 1]]) } },
        items: { t: "array", of: { t: "object", mode: "strict", shape: { n: { t: "number", checks: [["int"]] } } } },
      },
    },
    [
      { channel: { id: "123e4567-e89b-12d3-a456-426614174000", name: " x ", extra: 1 }, items: [{ n: 1 }] },
      { channel: { id: "nope", name: "" }, items: [{ n: 1.5 }, { n: 2, m: 3 }, "x"] },
      { channel: null },
      {},
    ],
  ],
  [
    "extend",
    { t: "extend", base: { t: "object", mode: "passthrough", shape: { a: { t: "string" }, b: { t: "number" } } }, shape: { b: { t: "string" }, c: { t: "boolean" } } },
    [{ a: "x", b: "y", c: true, z: 1 }, { a: "x", b: 1, c: true }],
  ],
  ["undefined", { t: "undefined" }, [U, null, "x"]],
  ["null", { t: "null" }, [null, U, 0]],
  ["unknown", { t: "unknown" }, [U, null, { a: [1, { b: 2 }] }, "x"]],
  ["meta.describe", str([["trim"]], [["meta", { description: "d" }], ["describe", "x"]]), [" a ", 1]],
  ["transformIdentity", str([["trim"], ["min", 1]], [["transformIdentity"]]), [" id ", ""]],
  ["refine.noWhitespace", str([["trim"], ["min", 1], ["max", 16]], [["refine", "noWhitespace"]]), ["👍", " 👍 ", "a b", "", 1]],
  ["refine.atHandle.optional", str([["trim"]], [["refine", "atHandle"], ["optional"]]), ["@alice", " @bob ", "alice", "@", "@ ", U, 3]],
  [
    "refine.titleOrDesc",
    { t: "object", mode: "passthrough", shape: { title: str([["trim"], ["min", 1]], [["optional"]]), description: str(null, [["optional"]]) }, wrap: [["refine", "titleOrDesc"]] },
    [{ title: "x" }, { description: "" }, {}, { title: "" }, { title: 5 }, null],
  ],
  [
    "superRefine.exactlyOne",
    { t: "object", mode: "passthrough", shape: { channel: str([["trim"], ["min", 1]], [["optional"]]), mine: { t: "enum", values: ["true", "false"], wrap: [["optional"]] } }, wrap: [["superRefine", "exactlyOne"]] },
    [{ channel: "#x" }, { mine: "true" }, {}, { channel: "#x", mine: "true" }, { mine: "false" }, { channel: 1 }, { channel: "" }],
  ],
  [
    "superRefine.pathIssue",
    { t: "object", shape: { a: { t: "string" }, b: { t: "string" } }, wrap: [["superRefine", "pathIssue"]] },
    [{ a: "ok", b: "ok" }, { a: "bad", b: "bad" }, { a: "bad", b: 1 }],
  ],
  [
    "refine.inner.abort",
    { t: "object", shape: { a: str([["min", 2]], [["refine", "atHandle"]]) }, wrap: [["refine", "titleOrDesc"]] },
    [{ a: 1 }, { a: "x" }, { a: "@x" }, { a: "@xy", title: 1 }],
  ],
  [
    "issues.order.multi",
    { t: "object", mode: "strict", shape: { s: str([["min", 3], ["regex", "^a"], ["max", 1]]), n: { t: "number", checks: [["int"], ["positive"], ["max", 0]] }, e: { t: "enum", values: ["x"] }, l: { t: "literal", value: "y" } } },
    [{ s: "bcd", n: 1.5, e: "q", l: "z", extra: 1, "0": 1 }, { s: "ab", n: -2 }],
  ],
];

const out = process.argv[2];
mkdirSync(dirname(out), { recursive: true });
const cases = [];
for (const [name, spec, inputs] of groups) {
  const schema = build(spec);
  for (const input of inputs) {
    const r = schema.safeParse(decode(input));
    cases.push(r.success
      ? { schema: name, input, success: true, data: encode(r.data) }
      : { schema: name, input, success: false, issues: JSON.parse(JSON.stringify(r.error.issues, (k, v) => (v === undefined ? { $undefined: true } : v))), message: r.error.message });
  }
}
const schemas = Object.fromEntries(groups.map(([name, spec]) => [name, spec]));
writeFileSync(out, `${JSON.stringify({ zod: zodVersion, node: process.version, schemas, cases }, null, 1)}\n`);
console.log(`zod ${zodVersion}: ${groups.length} schemas, ${cases.length} cases`);

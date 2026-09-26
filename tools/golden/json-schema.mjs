// Goldens for raft_shared::json_schema (decisions.md D15): Ajv 8 (2020-12) with
// the options manifestV1.ts uses, plus safe-regex2 verdicts, captured from the
// cli's locked copies.
// Usage: node tools/golden/json-schema.mjs tests/golden/raft-shared/json-schema.json
//
// compile: for each schema, `new Ajv2020({ allErrors: false, strict: false,
//   validateSchema: true, formats: {} }).compile(schema)` → ok, or the thrown
//   message. Console warnings Ajv logs during compile are recorded too.
// validate: for each compiled schema and payload, the boolean result and the
//   full `errors` array (Ajv stops at the first error with allErrors: false,
//   but applicator keywords can still leave several entries).
// safeRegex: safeRegex(pattern) for each pattern (the manifest bound check).
import { createRequire } from "node:module";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const require = createRequire(resolve(root, "upstream/raft-source/packages/cli/package.json"));
const Ajv2020 = require("ajv/dist/2020.js").default;
const safeRegex = require("safe-regex2");
const pkgVersion = (name) => {
  let dir = dirname(require.resolve(name));
  for (;;) {
    try {
      const pkg = JSON.parse(readFileSync(resolve(dir, "package.json"), "utf8"));
      if (pkg.name === name) return pkg.version;
    } catch {}
    dir = dirname(dir);
  }
};

const newAjv = () => new Ajv2020({ allErrors: false, strict: false, validateSchema: true, formats: {} });

const obj = (properties, extra = {}) => ({ type: "object", properties, ...extra });

// [name, schema, payloads]
const groups = [
  ["type.string", { type: "string" }, ["x", 1, null, true, [], {}]],
  ["type.integer", { type: "integer" }, [1, 1.5, "1", 1e21, -0]],
  ["type.number", { type: "number" }, [1.5, "1.5", null]],
  ["type.boolean", { type: "boolean" }, [true, 0]],
  ["type.null", { type: "null" }, [null, 0, ""]],
  ["type.array", { type: "array" }, [[], {}]],
  ["type.object", { type: "object" }, [{}, [], null]],
  ["enum", { enum: ["a", 1, null, { x: [1] }, [2]] }, ["a", 1, null, { x: [1] }, [2], "b", { x: [2] }, [2, 3]]],
  ["const", { const: { a: [1, "b"] } }, [{ a: [1, "b"] }, { a: [1] }, "x"]],
  ["const.number", { const: 1 }, [1, 1.0, "1"]],
  ["string.bounds", { type: "string", minLength: 2, maxLength: 3 }, ["a", "ab", "abcd", "😀", "😀😀", "😀a"]],
  ["string.pattern", { type: "string", pattern: "^[a-z]+$" }, ["abc", "aBc", ""]],
  ["string.pattern.unicode", { type: "string", pattern: "^.$" }, ["😀", "a", "ab"]],
  ["string.pattern.unanchored", { type: "string", pattern: "b" }, ["abc", "xyz"]],
  ["number.bounds", { type: "number", minimum: 1, maximum: 10 }, [0, 1, 10, 10.5]],
  ["integer.bounds", { type: "integer", minimum: 0, maximum: 100 }, [-1, 50, 101, 50.5]],
  ["array.items", { type: "array", items: { type: "string" }, minItems: 1, maxItems: 2 }, [[], ["a"], ["a", 1], ["a", "b", "c"], [1, 2, 3]]],
  [
    "object.required",
    obj({ a: { type: "string" }, b: { type: "integer" } }, { required: ["a", "b"] }),
    [{ a: "x", b: 1 }, { a: "x" }, {}, { b: 1 }, { a: 1, b: 1 }, { a: "x", b: "y" }, []],
  ],
  [
    "object.additional.false",
    obj({ a: { type: "string" } }, { additionalProperties: false }),
    [{ a: "x" }, { a: "x", z: 1 }, { z: 1, y: 2 }],
  ],
  [
    "object.additional.schema",
    obj({ a: { type: "string" } }, { additionalProperties: { type: "number" } }),
    [{ a: "x", n: 1 }, { a: "x", n: "y" }],
  ],
  [
    "object.nested",
    obj({ user: obj({ name: { type: "string", minLength: 1 }, tags: { type: "array", items: { enum: ["x", "y"] } } }, { required: ["name"] }) }, { required: ["user"] }),
    [{ user: { name: "n", tags: ["x"] } }, { user: { name: "" } }, { user: { tags: ["z"] } }, { user: { name: "n", tags: ["x", "q"] } }, { user: [] }],
  ],
  [
    "object.key.escaping",
    obj({ "a/b": { type: "string" }, "c~d": { type: "string" }, "e f": { type: "string" } }, { required: ["a/b"] }),
    [{ "a/b": 1 }, { "a/b": "x", "c~d": 1 }, { "a/b": "x", "e f": 1 }, {}],
  ],
  ["anyOf", { anyOf: [{ type: "string" }, { type: "integer", minimum: 5 }] }, ["x", 7, 3, null]],
  ["oneOf", { oneOf: [{ type: "integer" }, { type: "number", minimum: 2 }] }, [1, 3, 2.5, 1.5, "x"]],
  ["allOf", { allOf: [{ type: "string" }, { minLength: 2 }] }, ["ab", "a", 1]],
  ["not", { not: { type: "string" } }, [1, "x"]],
  [
    "defs.ref",
    { type: "object", $defs: { name: { type: "string", pattern: "^[a-z]+$" } }, properties: { first: { $ref: "#/$defs/name" }, list: { type: "array", items: { $ref: "#/$defs/name" } } } },
    [{ first: "abc", list: ["x"] }, { first: "ABC" }, { list: ["ok", "NO"] }],
  ],
  [
    "manifest.like",
    {
      type: "object",
      additionalProperties: false,
      required: ["channel", "count"],
      properties: {
        channel: { type: "string", minLength: 1, maxLength: 80 },
        count: { type: "integer", minimum: 1, maximum: 50 },
        mode: { enum: ["fast", "slow"] },
        filters: { type: "array", maxItems: 3, items: { type: "object", required: ["k"], properties: { k: { type: "string" }, v: { anyOf: [{ type: "string" }, { type: "null" }] } } } },
      },
    },
    [
      { channel: "c", count: 1 },
      { channel: "", count: 1 },
      { channel: "c", count: 0 },
      { channel: "c", count: 1, extra: true },
      { channel: "c", count: 1, mode: "medium" },
      { channel: "c", count: 1, filters: [{ k: "a", v: 1 }] },
      { channel: "c", count: 1, filters: [{}, {}, {}, {}] },
      { count: "1" },
    ],
  ],
  ["empty", {}, [1, "x", null, {}]],
  ["true.schema.items", { type: "array", items: true }, [[1, "x"]]],
  ["false.schema.prop", obj({ a: false }), [{}, { a: 1 }]],
];

// Schemas that should fail meta-schema validation or compilation.
const invalidSchemas = [
  ["bad.type", { type: "strin" }],
  ["bad.type.number", { type: 5 }],
  ["bad.minLength.negative", { type: "string", minLength: -1 }],
  ["bad.minLength.float", { type: "string", minLength: 1.5 }],
  ["bad.required.notArray", { type: "object", required: "a" }],
  ["bad.required.dupes", { type: "object", required: ["a", "a"] }],
  ["bad.properties.value", { type: "object", properties: { a: 5 } }],
  ["bad.enum.empty", { enum: [] }],
  ["bad.anyOf.empty", { anyOf: [] }],
  ["bad.items.array", { items: [{ type: "string" }] }],
  ["bad.pattern", { type: "string", pattern: "(" }],
  ["bad.ref.missing", { $ref: "#/$defs/nope" }],
  ["bad.additional", { additionalProperties: "no" }],
  ["bad.maximum.string", { maximum: "5" }],
  ["bad.nested", { type: "object", properties: { a: { type: "object", properties: { b: { minItems: "x" } } } } }],
  ["format.unknown", { type: "string", format: "email" }],
];

const safeRegexPatterns = [
  "^[a-z]+$", "(a+)+", "(a*)*b", "^(a|a)*$", "(x+x+)+y", "^\\d{3}-\\d{4}$", "(a|b|c)+", "((ab)*)+", "a{1,5}", "(a{1,30}){1,30}",
  "^([a-zA-Z0-9])(([\\-.]|[_]+)?([a-zA-Z0-9]+))*(@){1}[a-z0-9]+[.]{1}(([a-z]{2,3})|([a-z]{2,3}[.]{1}[a-z]{2,3}))$", "(.*a){20}", "[", "\\", "(?<n>a+)+", "(?:a+)+", "^(?:[a-z0-9]+(?:-[a-z0-9]+)*)$", "(a|aa)+", "x*y*z*", "(\\w+\\s?)+$",
];

const warnings = [];
const origWarn = console.warn;
const origLog = console.log;
const origError = console.error;

const compile = [];
const validate = [];
for (const [name, schema, payloads] of groups) {
  warnings.length = 0;
  console.warn = (...a) => warnings.push(a.map(String).join(" "));
  let fn;
  try {
    fn = newAjv().compile(schema);
  } finally {
    console.warn = origWarn;
  }
  compile.push({ name, schema, ok: true, warnings: [...warnings] });
  for (const payload of payloads) {
    const valid = fn(payload);
    validate.push({ name, payload, valid, errors: fn.errors ?? null });
  }
}
for (const [name, schema] of invalidSchemas) {
  warnings.length = 0;
  console.warn = (...a) => warnings.push(a.map(String).join(" "));
  console.log = (...a) => warnings.push(`log: ${a.map(String).join(" ")}`);
  console.error = (...a) => warnings.push(`error: ${a.map(String).join(" ")}`);
  let rec;
  try {
    newAjv().compile(schema);
    rec = { name, schema, ok: true };
  } catch (error) {
    rec = { name, schema, ok: false, error: `${error.constructor.name}: ${error.message}` };
  } finally {
    console.warn = origWarn;
    console.log = origLog;
    console.error = origError;
  }
  compile.push({ ...rec, warnings: [...warnings] });
}
const safe = safeRegexPatterns.map((pattern) => {
  try {
    return { pattern, safe: safeRegex(pattern) };
  } catch (error) {
    return { pattern, thrown: String(error) };
  }
});

const out = process.argv[2];
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, `${JSON.stringify({ ajv: pkgVersion("ajv"), safeRegex2: pkgVersion("safe-regex2"), node: process.version, compile, validate, safeRegex: safe }, null, 1)}\n`);
console.log(`ajv: ${compile.length} compiles, ${validate.length} validations, ${safe.length} safe-regex verdicts`);

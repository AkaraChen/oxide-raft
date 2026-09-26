// Golden records for raft_shared::js core helpers (decisions.md D3), taken
// from the running Node. See tests/golden/raft-shared/js-core.cmd.
//
// Encoding: every JS string is an array of UTF-16 code units, so lone
// surrogates survive JSON. Every JS value is a tagged record (see `enc`).
// Numbers that JSON cannot hold are strings ("NaN", "Infinity", "-Infinity",
// "-0").
import { writeFileSync } from "node:fs";

const u = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
const num = (n) => (Object.is(n, -0) ? "-0" : Number.isFinite(n) ? n : String(n));
const enc = (v) => {
  if (v === undefined) return { t: "undefined" };
  if (v === null) return { t: "null" };
  if (typeof v === "boolean") return { t: "bool", v };
  if (typeof v === "number") return { t: "number", v: num(v) };
  if (typeof v === "string") return { t: "string", v: u(v) };
  if (Array.isArray(v)) return { t: "array", v: Array.from(v, enc) };
  return { t: "object", v: Object.entries(v).map(([k, x]) => [u(k), enc(x)]) };
};
// For inputs whose key insertion order matters, pass entries explicitly.
const obj = (entries) => Object.fromEntries(entries);
const attempt = (fn) => {
  try {
    return { ok: fn() };
  } catch (err) {
    return { error: `${err.name}: ${err.message}` };
  }
};

const lone = "\ud83d";
const astral = "😀";
const strings = ["\u0085x\u0085", "\u180ex\u200b", "", "a", "abc", "héllo wörld", `a${astral}b`, `${astral}${astral}`, lone, `x${lone}y`, "\udc00z", " \t\n\u00a0\ufeff\u2028x\u3000 ", "aaaa111100000000"];

const numbers = [
  0, -0, 1, -1, 0.1, 0.5, 1.5, 2.5, -2.5, -0.5, 0.49999999999999994, 100, 123.456, 1 / 3, 2 / 3, 1e-6, 1e-7, 1.5e-7,
  1.2345e-10, 1e20, 1e21, 1e22, 1.5e300, 2 ** 53, 2 ** 53 + 2, 9007199254740993, 123456789012345680000,
  12345678901234567890, 5e-324, Number.MAX_VALUE, 0.000001, 0.0000012345, 1e100, 4.35, 1.005, 0.1 + 0.2,
  -1e-7, 25e20, 999999999999999900000, 1e21 + 1, NaN, Infinity, -Infinity,
];

const numericStrings = [
  "", " ", "0", "-0", "+1", "1.", ".5", "1e2", "1E-2", "0x10", "0X1f", "0b101", "0o17", "-0x10", "+0x10",
  "Infinity", "-Infinity", "+Infinity", "infinity", "1_000", "12abc", " \n\t42\u00a0", "\ufeff7\u2028", "1e400",
  "9007199254740993", "123456789012345678901", "0.1e-5", "abc", ".", "e5", "1e", "00012", "0x", "\u180e1",
  "\u00851", "1\u0085", "5.9", "1e25", "  -0x1F", "0x1F", "08", "0.0000001", "-", "+", "1e+2", "1e-400", "١٢",
];

const displayValues = [
  undefined, null, true, false, 0, -0, 1e21, 1.5e-7, "s", lone, [], [1, 2], [null, undefined, 3], [[1, [2]], "x"],
  {}, { a: 1 }, [{}], [lone],
];

const stringifyValues = [
  obj([["channel", "#x"], ["task_numbers", [87, 93]], ["z", 1]]),
  obj([["a", undefined], ["b", [undefined, null, NaN, Infinity, -0]], ["c", "q\"\\\b\f\n\r\t\u0001\u001f\u007f\u2028\u2029"]]),
  obj([["b", 1], ["2", "two"], ["a", 2], ["1", "one"], ["01", 3], ["4294967295", 4], ["4294967294", 5], ["-1", 6], ["1.5", 7]]),
  [astral, lone, `${lone}x`, `x\udc00`, "\ud83d\ude00"],
  obj([["n", [1e21, 1e-7, 0.1, 123456789012345680000, 5e-324]]]),
  {}, [], [[]], [{}], obj([["e", {}], ["f", []], ["g", [[], {}]]]),
  "top", 5, null, true,
  obj([["deep", obj([["deeper", [1, obj([["x", undefined]]), "y"]]])]]),
];

const jsonTexts = [
  "{}", "[]", "null", "true", "1", "-0", "1e400", "12345678901234567890", "0.1", "\"a\\u00e9\\ud83d\\ude00\"",
  "\"\\ud800\"", "\"\\udc00x\"", "{\"b\":1,\"2\":2,\"a\":3,\"1\":4}", "{\"a\":1,\"a\":2}", " [1 , 2 ] ", "\ufeff{}",
  "{\"__proto__\":1}", "\"\\/\\b\\f\\n\\r\\t\"",
  "", " ", "nope", "{", "[1,]", "{\"a\":1,}", "{a:1}", "{'a':1}", "[1 2]", "\"\\x\"", "\"abc", "01", "1.", ".5", "-",
  "tru", "nul", "{\"a\" 1}", "[\"a\"\n,]", "\"\u0001\"", "{\"a\":1}x", "123abc", "NaN", "Infinity", "[", "\"\\u12\"",
  `{"message":"${"x".repeat(40)}",}`, "{\n  \"a\": 1,\n  \"b\"\n}",
];

const slices = [
  ["aaaa111100000000", 0, 8], ["abc", 0, undefined], ["abc", -2, undefined], ["abc", 1, -1], ["abc", 5, 10],
  ["abc", 2, 1], [`a${astral}b`, 0, 2], [`a${astral}b`, 2, 4], [`${astral}${astral}`, 1, 3], ["", 0, 8],
  ["héllo wörld", 3, 8], ["abc", -10, 2], ["abc", -Infinity, Infinity], ["abc", NaN, 2],
];

const pads = [["5", 3, "0"], ["abc", 2, "0"], ["x", 6, "ab"], ["x", 4, ""], [astral, 4, "-"], ["x", 3, lone]];

const splits = [
  ["a:b:c", ":", undefined], ["a:b:c", ":", 2], ["a:b:c", ":", 0], ["a::b", ":", undefined], ["", ":", undefined],
  ["abc", "", undefined], ["abc", "", 2], [`a${astral}b`, "", undefined], ["a:b", "x", undefined], ["a,b,c", ",", -1],
];

const replaces = [
  ["a.b.c", ".", "-"], ["a.b.c", ".", "$&$&"], ["a.b.c", ".", "$$"], ["a.b.c", ".", "[$`|$']"], ["a.b.c", ".", "$1"],
  ["aaa", "", "-"], ["abc", "x", "-"], ["a$b", "$", "$$"], ["abc", "b", "$0$&$9$<n>"], ["\r\n\r\n", "\r\n", "\n"],
];

const uriComponents = ["a b&c=d/e?f#g", "é😀", "-_.!~*'()", lone, "a\udc00", "\u0000\u007f", "%"];

const intInputs = [
  ["5.9", undefined], ["1e25", undefined], ["  -0x1F", undefined], ["0x1F", 16], ["0x1F", 10], ["08", undefined],
  ["ff", 16], ["z", 36], ["10", 2], ["12", 1], ["12", 37], ["", undefined], ["abc", undefined], ["123456789012345678901234", undefined],
  ["-0", undefined], ["+7", undefined], ["\u00a0 42", undefined], ["1_000", undefined], ["0b11", undefined], ["Infinity", undefined],
];

const roundInputs = [-2.5, -1.5, -0.5, 0.5, 1.5, 2.5, 0.49999999999999994, -0, NaN, Infinity, 2 ** 53, -(2 ** 52) - 0.5, 1.4999999999999998];
const fixedInputs = [[1.005, 2], [-0, 2], [-0.0001, 2], [1e21, 2], [123.456, 0], [0.5, 0], [1.5, 0], [2.5, 0], [-1.5, 0], [NaN, 2], [1.45, 1], [0.000001, 7], [99.995, 2]];
const floorDivInputs = [[7, 2], [-7, 2], [7, -2], [-7, -2], [1, 0], [-1, 0], [0, 0], [5.5, 2], [2 ** 53, 3]];
const integerOrInfinity = [NaN, 0, -0, 1.9, -1.9, Infinity, -Infinity, 1e300];
const safeIntegers = [0, -0, 1.5, 2 ** 53 - 1, 2 ** 53, -(2 ** 53 - 1), -(2 ** 53), NaN, Infinity];

const toNumberValues = [undefined, null, true, false, 5, "0x10", " 12 ", "", "x", [], [5], [1, 2], [null], [" 7 "], {}, [[3]]];
const lessThan = [
  [1, 2], [2, 1], [1, 1], ["a", "b"], ["b", "a"], ["10", "9"], ["10", 9], [9, "10"], ["0x10", 17], [null, 1],
  [undefined, 1], [true, 2], [NaN, 1], ["abc", 1], [astral, "\uffff"], [[5], 6], [{}, 1], ["", "a"], ["a", "aa"],
];
const truthy = [undefined, null, 0, -0, NaN, "", "0", " ", [], {}, false, true, 1];

const sortInputs = [["b", "a", "B", "10", "9", "", astral, "\uffff", lone, "é", "e"]];

const assigns = [
  [obj([["a", 1], ["b", 2]]), obj([["b", undefined], ["c", 3]])],
  [obj([["b", 1], ["2", 2]]), obj([["1", 3], ["a", 4], ["b", 5]])],
  [{}, obj([["x", undefined]])],
];

const bodies = [[0xef, 0xbb, 0xbf, 0x7b, 0x7d], [0x61, 0xff, 0x62], [0xe2, 0x82], [0xef, 0xbb, 0xbf, 0xef, 0xbb, 0xbf, 0x61], []];

const golden = {
  to_number: numericStrings.map((s) => ({ input: u(s), output: num(Number(s)) })),
  number_to_string: numbers.map((n) => ({ input: num(n), output: u(String(n)) })),
  to_fixed: fixedInputs.map(([n, k]) => ({ input: num(n), digits: k, output: u(n.toFixed(k)) })),
  parse_int: intInputs.map(([s, r]) => ({ input: u(s), radix: r ?? null, output: num(r === undefined ? parseInt(s) : parseInt(s, r)) })),
  math_round: roundInputs.map((n) => ({ input: num(n), output: num(Math.round(n)) })),
  math_floor_div: floorDivInputs.map(([a, b]) => ({ a: num(a), b: num(b), output: num(Math.floor(a / b)) })),
  to_integer_or_infinity: integerOrInfinity.map((n) => ({ input: num(n), output: num(Number.isNaN(n) ? 0 : Math.trunc(n) + 0) })),
  is_safe_integer: safeIntegers.map((n) => ({ input: num(n), output: Number.isSafeInteger(n), integer: Number.isInteger(n) })),
  utf16_len: strings.map((s) => ({ input: u(s), output: s.length })),
  utf16_slice: slices.map(([s, a, b]) => ({ input: u(s), start: num(a), end: b === undefined ? null : num(b), output: u(s.slice(a, b)) })),
  trim: strings.map((s) => ({ input: u(s), trim: u(s.trim()), trim_start: u(s.trimStart()), trim_end: u(s.trimEnd()) })),
  pad: pads.map(([s, n, f]) => ({ input: u(s), length: n, fill: u(f), start: u(s.padStart(n, f)), end: u(s.padEnd(n, f)) })),
  split: splits.map(([s, sep, limit]) => ({ input: u(s), sep: u(sep), limit: limit ?? null, output: s.split(sep, limit).map(u) })),
  replace: replaces.map(([s, p, r]) => ({ input: u(s), pattern: u(p), replacement: u(r), first: u(s.replace(p, r)), all: u(s.replaceAll(p, r)) })),
  encode_uri_component: uriComponents.map((s) => ({ input: u(s), ...(() => { const r = attempt(() => encodeURIComponent(s)); return r.error ? { error: r.error } : { output: u(r.ok) }; })() })),
  to_display_string: displayValues.map((v) => ({ input: enc(v), output: u(String(v)) })),
  is_truthy: truthy.map((v) => ({ input: enc(v), output: Boolean(v) })),
  to_number_value: toNumberValues.map((v) => ({ input: enc(v), output: num(Number(v)) })),
  less_than: lessThan.map(([a, b]) => ({ a: enc(a), b: enc(b), output: a < b })),
  json_stringify: stringifyValues.map((v) => ({ input: enc(v), output: u(JSON.stringify(v)), pretty: u(JSON.stringify(v, null, 2)) })),
  json_parse: jsonTexts.map((text) => {
    const r = attempt(() => JSON.parse(text));
    return r.error ? { input: u(text), error: r.error } : { input: u(text), output: enc(r.ok) };
  }),
  response_text: bodies.map((bytes) => ({ input: bytes, output: u(new TextDecoder().decode(new Uint8Array(bytes))) })),
  sort_default: sortInputs.map((list) => ({ input: list.map(u), output: [...list].sort().map(u) })),
  object_assign: assigns.map(([a, b]) => ({ a: enc(a), b: enc(b), output: enc(Object.assign({}, a, b)), spread: enc({ ...a, ...b }) })),
  object_from_entries: [
    [["b", 1], ["2", "two"], ["a", 2], ["1", "one"], ["01", 3], ["4294967295", 4], ["4294967294", 5], ["-1", 6], ["1.5", 7], ["0", 0]],
    [["x", 1], ["y", 2], ["x", 3]],
  ].map((entries) => ({ input: entries.map(([k, v]) => [u(k), enc(v)]), output: enc(Object.fromEntries(entries)) })),
  error_to_string: [["Error", "boom"], ["TypeError", "fetch failed"], ["RangeError", ""]].map(([name, message]) => {
    const err = new Error(message);
    err.name = name;
    return { name, message, output: String(err) };
  }),
};

writeFileSync(process.argv[2], `${JSON.stringify(golden, null, 1)}\n`);

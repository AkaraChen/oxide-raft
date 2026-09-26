// Goldens for raft_shared::js dates, time zones and collation (decisions.md D3,
// D23). Each time zone runs in its own child process with TZ set, because V8
// reads TZ once at start-up.
// Usage: node tools/golden/dates.mjs tests/golden/raft-shared/dates.json
//
// Per TZ (see ZONES; "" = TZ unset is not used: the container default differs
// by machine):
//   resolvedTimeZone   Intl.DateTimeFormat().resolvedOptions().timeZone
//   parse              Date.parse(s) for each string of PARSE_INPUTS (NaN → null)
//   local              for each ms of LOCAL_MS: getTimezoneOffset and the local
//                      field getters (year, month, date, hours, minutes,
//                      seconds, milliseconds, day)
//   construct          new Date(y, mo, d, h, mi, s, ms).getTime() for LOCAL_FIELDS
// Zone independent:
//   isoString          new Date(ms).toISOString() or the thrown RangeError text
//   validZones         whether new Intl.DateTimeFormat("en-US", { timeZone }) accepts
//                      each id, and its resolvedOptions().timeZone when it does
//   localeDate         toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" })
//   localeCompare      a.localeCompare(b) sign for each pair of COLLATE_INPUTS,
//                      and the default-locale sort of COLLATE_INPUTS
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

// Collation depends on the locale (LC_ALL, then LC_MESSAGES, then LANG);
// goldens are captured with all three unset, which Node resolves to en-US (D24).
const isChild = process.argv[2]?.startsWith("--") ?? false;
const localeUnset = process.env.LANG === undefined && process.env.LC_ALL === undefined && process.env.LC_MESSAGES === undefined;
if (!isChild && (!localeUnset || new Intl.Collator().resolvedOptions().locale !== "en-US")) {
  throw new Error("run with LANG, LC_ALL and LC_MESSAGES unset (decisions.md D24)");
}

const ZONES = ["Asia/Shanghai", "UTC", "America/New_York", "Europe/London", "Australia/Lord_Howe", "Asia/Kolkata", "America/Sao_Paulo", "Etc/GMT+5", "asia/shanghai", "Invalid/Zone", "EST5EDT", ":Asia/Tokyo"];

const PARSE_INPUTS = [
  "2026-04-21T07:00:00Z", "2026-04-21T07:00:00.123Z", "2026-04-21T07:00:00.123456Z", "2026-04-21T07:00:00+08:00", "2026-04-21T07:00:00-0530",
  "2026-04-21T07:00:00", "2026-04-21T07:00", "2026-04-21", "2026-04", "2026", "+002026-04-21T07:00:00Z", "-000001-01-01T00:00:00Z",
  "2026-04-21 07:00:00", "2026-04-21 07:00:00Z", "2026/04/21 07:00:00", "2026/04/21", "04/21/2026", "Apr 21 2026", "21 Apr 2026 07:00 GMT",
  "Tue, 21 Apr 2026 07:00:00 GMT", "Tue Apr 21 2026 15:00:00 GMT+0800 (China Standard Time)", "2026-02-30T00:00:00Z", "2026-13-01", "2026-04-21T24:00:00Z",
  "2026-04-21T07:00:00.1Z", "2026-04-21T07:00:60Z", "", " ", "x", "1776754800000", "2026-04-21T07:00:00z", "2026-04-21t07:00:00Z",
  "2026-03-08T02:30:00", "2026-11-01T01:30:00", "2026-03-29T01:30:00", "2026-10-25T01:30:00", "275760-09-13T00:00:00Z", "+275760-09-13T00:00:00.001Z",
];

const LOCAL_MS = [0, -1, 1776754800000, 1772953200000, 1772956800000, 1793509200000, 1774747800000, 1792884600000, 951782400000, -62198755200000, 8.64e15];

const LOCAL_FIELDS = [
  [2026, 3, 21, 15, 0, 0, 0], [2026, 0, 1, 0, 0, 0, 0], [2026, 1, 30, 0, 0, 0, 0], [2026, 2, 8, 2, 30, 0, 0], [2026, 10, 1, 1, 30, 0, 0],
  [2026, 2, 29, 1, 30, 0, 0], [2026, 9, 25, 1, 30, 0, 0], [2026, 12, 1, 0, 0, 0, 0], [2026, 3, 21, 24, 0, 0, 0], [2026, 3, 21, 15, 0, 0, 999],
  [99, 0, 1, 0, 0, 0, 0], [1970, 0, 1, 0, 0, 0, 0], [275760, 8, 13, 0, 0, 0, 0],
];

const ISO_MS = [0, -1, 1776754800123, -62198755200000, -62198755200001, 8.64e15, -8.64e15, 8.64e15 + 1, NaN, 253402300800000];

const VALID_ZONES = ["Asia/Shanghai", "asia/shanghai", "ASIA/SHANGHAI", "UTC", "utc", "Etc/UTC", "GMT", "Etc/GMT+5", "US/Eastern", "Asia/Calcutta", "Asia/Kolkata", "America/Buenos_Aires", "EST", "EST5EDT", "Invalid/Zone", "", "+08:00", "-0530", "Z", "Asia/Shanghai ", "Europe/Kyiv", "Europe/Kiev", "Asia/Saigon", "Asia/Ho_Chi_Minh"];

const LOCALE_DATE_MS = [0, 1776754800000, 1772953200000, -62198755200000, 1767225599999];

const COLLATE_INPUTS = ["a", "A", "b", "B", "ä", "a1", "a10", "a2", "_x", "-x", "x-", "x_", "Zeta", "zeta", "éclair", "eclair", "中文", "日本", "😀", "", " ", "a b", "ab", "a.b", "a/b", "a\\b", "file.md", "File.md", "readme", "README", "résumé", "resume"];

// Locale matrix (D24): per locale environment, Node's default collator locale
// and the sorted order of LOCALE_SORT_INPUTS (a sample of CJK ideographs, kana,
// Hangul and the Latin collation inputs), which fixes every pairwise order.
const LOCALE_ENVS = [
  {}, { LANG: "" }, { LANG: "C" }, { LANG: "en_US.UTF-8" }, { LANG: "da_DK.UTF-8" }, { LANG: "da__DK" }, { LANG: "sv_SE.UTF-8" },
  { LANG: "sv_SE.UTF-8@collation=phonebook" }, { LANG: "ja_JP.UTF-8" }, { LANG: "ja_JP@calendar=japanese" }, { LANG: "zh_CN.UTF-8" },
  { LANG: "zh_TW.UTF-8" }, { LANG: "zh_Hant_TW.UTF-8" }, { LANG: "ko_KR.UTF-8" }, { LANG: "ko_KR@collation=search" }, { LANG: "tl_PH.UTF-8" },
  { LANG: "es_ES@traditional" }, { LANG: "de_DE@euro" }, { LANG: "se_NO.UTF-8" }, { LANG: "haw_US.UTF-8" }, { LANG: "tr_TR.UTF-8" },
  { LC_ALL: "de_DE.UTF-8", LANG: "da_DK.UTF-8" }, { LC_MESSAGES: "sv_SE.UTF-8", LANG: "en_US.UTF-8" }, { LC_ALL: "", LANG: "da_DK.UTF-8" },
  { LC_COLLATE: "da_DK.UTF-8" }, { LANGUAGE: "da" },
];
const cjk = [];
for (let cp = 0x4e00; cp <= 0x9fff; cp += 11) cjk.push(String.fromCodePoint(cp));
for (let cp = 0x3041; cp <= 0x3096; cp += 3) cjk.push(String.fromCodePoint(cp));
for (let cp = 0x30a1; cp <= 0x30fa; cp += 3) cjk.push(String.fromCodePoint(cp));
for (let cp = 0xac00; cp <= 0xd7a3; cp += 97) cjk.push(String.fromCodePoint(cp));
const LOCALE_SORT_INPUTS = [...cjk, "缵", "纓", "中文", "日本", "東京", "北京", "aa", "å", "z", "ä", "ö", "ø", "æ", "ü", "ı", "i", "I", "İ", "ch", "c", "d", "ll", "l", "m", "ñ", "n", "ß", "ss", "v", "w", "Å", "Ä", "Ö"];

if (process.argv[2] === "--locale-child") {
  process.stdout.write(JSON.stringify({
    locale: new Intl.Collator().resolvedOptions().locale,
    sorted: [...LOCALE_SORT_INPUTS].sort((a, b) => a.localeCompare(b)),
  }));
  process.exit(0);
}

if (process.argv[2] === "--child") {
  const zone = {};
  zone.resolvedTimeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  zone.parse = PARSE_INPUTS.map((s) => {
    const v = Date.parse(s);
    return Number.isNaN(v) ? null : v;
  });
  zone.local = LOCAL_MS.map((ms) => {
    const d = new Date(ms);
    if (Number.isNaN(d.getTime())) return null;
    return [d.getTimezoneOffset(), d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds(), d.getDay()];
  });
  zone.construct = LOCAL_FIELDS.map((f) => {
    const t = new Date(...f).getTime();
    return Number.isNaN(t) ? null : t;
  });
  process.stdout.write(JSON.stringify(zone));
  process.exit(0);
}

const zones = {};
for (const tz of ZONES) {
  const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), "--child"], { env: { PATH: process.env.PATH, TZ: tz }, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`child for ${tz} failed: ${r.stderr}`);
  zones[tz] = JSON.parse(r.stdout);
}

const isoString = ISO_MS.map((ms) => {
  try {
    return { ms: Number.isNaN(ms) ? null : ms, iso: new Date(ms).toISOString() };
  } catch (e) {
    return { ms: Number.isNaN(ms) ? null : ms, error: `${e.name}: ${e.message}` };
  }
});
const validZones = VALID_ZONES.map((timeZone) => {
  try {
    const f = new Intl.DateTimeFormat("en-US", { timeZone });
    f.format(0);
    return { timeZone, valid: true, resolved: f.resolvedOptions().timeZone };
  } catch (e) {
    return { timeZone, valid: false, error: `${e.name}: ${e.message}` };
  }
});
const localeDate = LOCALE_DATE_MS.map((ms) => ({ ms, text: new Date(ms).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" }) }));
const localeMatrix = LOCALE_ENVS.map((vars) => {
  const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), "--locale-child"], { env: { PATH: process.env.PATH, ...vars }, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`locale child for ${JSON.stringify(vars)} failed: ${r.stderr}`);
  return { env: vars, ...JSON.parse(r.stdout) };
});
const localeCompare = [];
for (const a of COLLATE_INPUTS) for (const b of COLLATE_INPUTS) localeCompare.push(Math.sign(a.localeCompare(b)));
const sorted = [...COLLATE_INPUTS].sort((a, b) => a.localeCompare(b));

const out = process.argv[2];
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, `${JSON.stringify({
  node: process.version,
  icu: process.versions.icu,
  tz: process.versions.tz,
  inputs: { ZONES, PARSE_INPUTS, LOCAL_MS, LOCAL_FIELDS, COLLATE_INPUTS },
  zones,
  isoString,
  validZones,
  localeDate,
  localeCompare,
  sorted,
  LOCALE_SORT_INPUTS,
  localeMatrix,
}, null, 1)}\n`);
console.log(`dates: ${ZONES.length} zones, ${PARSE_INPUTS.length} parse inputs, ${COLLATE_INPUTS.length ** 2} collation pairs`);

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
}, null, 1)}\n`);
console.log(`dates: ${ZONES.length} zones, ${PARSE_INPUTS.length} parse inputs, ${COLLATE_INPUTS.length ** 2} collation pairs`);

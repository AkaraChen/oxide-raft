# Research: `@botiverse/raft-shared`, `raft-trace-client`, `@botiverse/oar` (for the Rust port)

Upstream pin: `upstream/raft-source` @ `05f7d8f`; oar: `upstream/oar` @ tag `v0.0.7` (`a1a9857`).
All paths below are relative to `upstream/raft-source/packages/` unless prefixed `oar/` (= `upstream/oar/packages/oar/`).

Method: every `import … from "@botiverse/raft-shared[/src/…]"` (including multi-line imports) in non-test
`.ts` files of `cli/src`, `computer/src`, `daemon/src` was extracted. Excluded: `*.test.ts`, `*.typeproof.ts`,
`__tests__/`, and daemon pi / kimi-sdk-only drivers (`drivers/pi.ts`, `piCommandTool.ts`,
`piToolExecutionObservability.ts`, `piEventNormalizer.ts`, `kimi-sdk.ts`). Each symbol was resolved
through `shared/src/index.ts` re-exports (`export *`, `export {…} from`) to its defining declaration with the
TypeScript AST (typescript 5.8.3, syntax-only). A **symbol-level** closure was then computed: for each needed
top-level declaration, every identifier it references (in values *and* type annotations) that resolves to
another top-level declaration or import was pulled in transitively. "Decl-lines" = the summed line spans of
the needed declarations (JSDoc included). This slightly over-approximates, because a property name that
matches a top-level name also counts as a reference. It is far tighter than whole-file counting.

---

## 1. Symbols imported from raft-shared, and the minimal closure

### 1.1 Counts

| Consumer | distinct symbols | closure decl-lines | files touched | (those files' total lines) |
|---|---|---|---|---|
| cli | 122 | 6,591 | 25 | 12,429 |
| computer | 8 | 147 | 3 | 4,461 |
| daemon (excl. pi/kimi-sdk) | 146 (153 before exclusion) | 5,362 | 29 | 12,256 |
| trace-client (needs from shared) | 10 | 274 | 1 | 626 |
| **union** | **264** | **11,185** | **43** | **15,794** |
| union minus agent-migration symbols | 247 | 11,091 | 41 | 15,626 |

Excluding pi/kimi-sdk drops only these daemon symbols: `BUILTIN_RUNTIME_HOST_PROVIDER_ENV_SCRUB_KEYS`,
`PI_BUILTIN_PROVIDER_ENV_KEYS`, `ProviderConnectionLaunchProjection`, `RUNTIME_MODELS`, `buildLaunchPlan`,
`getRuntimeProviderDisplayName`, `humanizeRuntimeProviderSegment`.

**sync-core is NOT needed.** No needed symbol resolves into `@botiverse/raft-sync-core`. `index.ts:242` does
`export * from "@botiverse/raft-sync-core"`, but only `discussionGraph.ts`, `canonicalMessageV2.ts` and
`inboxScopeReadFrontier.ts` import it, and none of them is in the closure. Do not fold sync-core in.

The shared `package.json` depends only on `zod ^4.3.6` and `@botiverse/raft-sync-core`.

### 1.2 Per-file closure (union)

| shared/src file | needed decls | decl-lines | file lines |
|---|---|---|---|
| agentApiContract.ts | 320 | 2,649 | 3,195 |
| index.ts | 174 | 1,887 | 3,807 |
| piBuiltinModels.generated.ts | 4 | 607 | 661 |
| tracing/index.ts | 69 | 567 | 626 |
| toolDisplay.ts | 26 | 496 | 534 |
| raftRefs.ts | 40 | 496 | 544 |
| daemonApiRawClient.ts | 32 | 494 | 549 |
| apmHeldFreshness.ts | 23 | 331 | 353 |
| runtimeConfigLegacy.ts | 16 | 322 | 369 |
| agentApiRawClient.ts | 24 | 320 | 368 |
| agentApiMessageContract.ts | 33 | 284 | 339 |
| daemonApiContract.ts | 37 | 266 | 301 |
| agentApiClient.ts | 24 | 251 | 299 |
| daemonApiClient.ts | 24 | 239 | 289 |
| actionCards.ts | 15 | 205 | 522 |
| externalAgentIntegration.ts | 29 | 174 | 229 |
| agentApiChannelJoin.ts | 11 | 159 | 170 |
| agentInbox.ts | 17 | 155 | 174 |
| runtimeAccountUsage.ts | 14 | 145 | 168 |
| agentInboxApp.ts | 15 | 115 | 156 |
| appConfigTransport.ts | 8 | 99 | 107 |
| capabilityInventories.ts | 14 | 95 | 106 |
| appRuntimeTrace.ts | 8 | 92 | 138 |
| apps/reminder/protocol.ts | 3 | 83 | 93 |
| thirdPartyInertRenderer.ts | 4 | 71 | 125 |
| attachmentUploadContract.ts | 12 | 70 | 252 |
| runtimeProviderDisplay.ts | 4 | 64 | 73 |
| agentMigrationResumable.ts | 13 | 61 | 93 |
| knowledgeContext.ts | 9 | 53 | 65 |
| managedMcp.ts | 7 | 49 | 140 |
| apps/cleaner/configProtocol.ts | 8 | 45 | 86 |
| oauthClientCategories.ts | 5 | 40 | 45 |
| agentMigration.ts | 8 | 32 | 75 |
| brandedIds.ts | 8 | 29 | 85 |
| typeGuards.ts | 1 | 26 | 29 |
| attentionDependencyOracle.ts | 4 | 25 | 102 |
| generated/runtimeProviderDisplayNames.ts | 1 | 21 | 28 |
| providerConnections.ts | 6 | 18 | 57 |
| externalProjection.ts | 2 | 16 | 303 |
| utcTimestamp.ts | 1 | 13 | 14 |
| clock.ts | 4 | 12 | 28 |
| producerFactLineage.ts | 3 | 8 | 93 |
| agentApiPaths.ts | 1 | 1 | 4 |

Notes on what drives the size:
- **The CLI imports the `agentApiContract` *value*** (`cli/src/agentApiPath.ts:4`), which is the whole
  77-route table (`agentApiContract.ts:1712-2489`, 778 lines). Through it the CLI reaches every request and
  response zod schema in the file (≈2.6k lines). This is the bulk of the port.
- **`piBuiltinModels.generated.ts` (607 lines) is reachable only through `RUNTIME_MODELS.builtin`**
  (`index.ts:1913`). The daemon's only live path into it is `getStaticRuntimeModelSourceSet`
  (`index.ts:2036`, called at `daemon/src/core.ts:3998`), and that path serves only
  `STATIC_RUNTIME_MODEL_SOURCE_IDS = ["claude","copilot","gemini"]` (`index.ts:1885`). With `builtin` dropped,
  Rust needs only the 18-line `PI_BUILTIN_PROVIDER_API_KEY_ENV_KEYS_GENERATED` map
  (`piBuiltinModels.generated.ts:18`). The daemon still uses it through `BUILTIN_RUNTIME_PROVIDER_ENV_KEYS`
  (`index.ts:1003`) and `providerConnectionLaunch.ts:2-7`, a non-pi path. The realistic closure is therefore
  **≈10.5k decl-lines**.
- `hydrateRuntimeConfig` / `runtimeConfigToLaunchFields` (used by 15 drivers) pull in
  `runtimeConfigLegacy.ts` (322 lines) through `hydrateLegacyRuntimeConfigWithTrace` (`index.ts:13`).
- Agent migration: if "legacy migration" in the scope decision includes agent *workspace* migration
  (`daemon/src/agentMigration*.ts`), the 17 `AGENT_MIGRATION_*` / `AgentMigration*` /
  `agentMigrationTransferSummarySchema` symbols go away (−94 decl-lines). The CLI still uses the agent-api
  `migrationBegin/Status/Ready/Arrived` routes (`cli/src/commands/migrate/index.ts:6`), which live in
  `agentApiContract`. This needs a decision.
- Computer's dropped k-carrier/update/legacy files (`kHostAdapter.ts`, `kReleaseSource.ts`,
  `kUpgradeProcess.ts`, `legacySupervisorTakeover.ts`, `macosLoginCarrier.ts`, …) import only clock helpers,
  so dropping them removes no shared surface. Computer's whole need is `Tracer`, `noopTracer`,
  `ComputerLifecycleAction`, `ComputerLifecycleExecutionAck` and the 4 clock fns.

### 1.3 Symbol table

The full table (264 rows: symbol → defining file:line, kind, consumers) is in the **Appendix**. Kind
counts: ~150 types/interfaces, ~55 consts (7 of them zod schemas), ~55 functions, 1 class (`BasicTracer`).
Subpath (`@botiverse/raft-shared/src/...`) imports are marked "(subpath)"; see §8.

---

## 2. Runtime use of zod, and the Rust mapping

### 2.1 Where zod actually executes

| Place | What is parsed | Effect |
|---|---|---|
| `agentApiRawClient.ts:139` (`encodePathParams`) | route `request.params` | **parsed output** is interpolated into the path |
| `agentApiRawClient.ts:167` (`encodeQuery`) | route `request.query` | **parsed output** is serialized to `URLSearchParams` |
| `agentApiRawClient.ts:190` (`parseBody`) | route `request.body` | **parsed output is what gets sent** (`:251-256`) |
| `agentApiContract.ts:3160-3175` (`parseAgentApiResponse`) | every JSON response body | `response.body.parse(value)`; the caller receives the parsed value |
| `agentApiContract.ts:3186-3194` (`parseAgentApiAppSourceAckReject`) | error body (daemon `agentCredentialProxy.ts`) | `safeParse`, plus a status↔code table |
| `daemonApiRawClient.ts` (`encodeQuery` ~`:362`, body ~`:398`, response) | CLI→daemon routes | parse, and on `ZodError` builds a `DaemonApiContractRejectionDiagnostic` from `issues[0]` (`:308-351`) |
| `cli/src/commands/action/prepare.ts:129-134` | `actionCardActionSchema.safeParse(stdin JSON)` | **zod issue messages printed to the agent**: ``${path.join(".")\|\|"(root)"}: ${message}`` joined by `; ` |
| `cli/src/commands/message/_continueDraftState.ts:54` | `agentApiStructuredMentionSchema.safeParse` on a local draft file | silently drops invalid items |
| `cli` external adapters: `validateExternalAgentWakeEventEnvelope` / `validateExternalRuntimeIntegrationManifest` (`externalAgentIntegration.ts:195-203`) | wake events and manifests | `.strict()` schemas with `.parse` |
| `cli/src/commands/knowledge/context.ts:27,36-37` → `validateKnowledgeContext` | CLI args | length checks (not zod) |
| `daemon/src/agentMigrationExport.ts:166`, `agentMigrationResumableBundle.ts:364` | `agentMigrationTransferSummarySchema` (`.strict().superRefine`) | parse / safeParse |

**Not validated:** server↔daemon WebSocket messages. `daemon/src/connection.ts:277` does
`const msg: ServerToMachineMessage = JSON.parse(data.toString())`, a bare cast. `ServerToMachineMessage`
(`index.ts:545-699`, 155 lines) and `MachineToServerMessage` (`index.ts:775-903`, 129 lines) are plain TS
unions with no schemas. Rust must decode them **tolerantly**: an unknown `type` or unknown fields must not
kill the connection (TS just falls through the switch). Use a `#[serde(tag="type")]` enum with an
`#[serde(other)] Unknown` arm, or decode to `Value` first.

The daemon package itself imports zod only in `drivers/codex.ts` and `drivers/codexInstructionShape.ts`
(local, not shared). `daemon/package.json` also lists `zod ^4.3.6`.

### 2.2 zod modes in the closure (zod **v4.3.6**)

| file | `z.object` (strips unknown keys) | `passthroughObject` / `.passthrough()` (keeps unknown keys) | `.strict()` / `z.strictObject` (rejects unknown keys) |
|---|---|---|---|
| agentApiContract.ts | 27 | 155 + 3 | 10 |
| agentApiMessageContract.ts | 4 | 8 + 3 | 1 |
| daemonApiContract.ts | 2 | 16 + 1 | 2 |
| actionCards.ts | 7 | 0 | 0 |
| externalAgentIntegration.ts | 7 | 0 | 7 |
| agentMigration.ts | 4 | 0 | 4 |
| attachmentUploadContract.ts | 0 | 0 | 10 (`strictObject`) |
| runtimeAccountUsage.ts | 0 | 0 | 3 (`strictObject`) |

`passthroughObject = (shape) => z.object(shape).passthrough()` (`agentApiContract.ts:164`,
`agentApiMessageContract.ts:10`, and the equivalent in `daemonApiContract.ts`). Almost all agent-api
request/response objects are **passthrough**, so unknown server fields survive parsing and are handed to the
CLI formatters (and to any `--json` output). Strict examples: the feedback-locator schemas
(`agentApiContract.ts:120-162`), `AgentInboxSourceRef` (`:596`), the app-config patch body (`:1588-1600`),
daemon inbox ack (`daemonApiContract.ts:120-122`), and all of `externalAgentIntegration.ts` and
`agentMigration.ts`.

### 2.3 Parse-time *transforms* (they change what goes on the wire)

Because `requestAgentApiRawRoute` sends the *parsed* body/query/params, these rewrite the outgoing request:
- `.trim()` on strings. There are 100 uses in `agentApiContract.ts`, 28 in `actionCards.ts`, 21 in
  `daemonApiContract.ts`, 8 in `agentApiMessageContract.ts` and 4 in `runtimeAccountUsage.ts`. In zod v4,
  `.trim()` mutates the output.
- `.default(...)`: `actionCards.ts:57` (`visibility` → `"public"`), `:175` (`scopes` → `[]`),
  `agentApiContract.ts:1598-1599` (app-config patch `set: {}`, `unset: []`). Defaults are materialized into
  the sent JSON.
- `.transform(asMessageId / asChannelId)` (`agentApiContract.ts:341,345,355,796,1308`). These are identity at
  runtime (branding only).
- `z.coerce.number()` for `task_number` (`agentApiContract.ts:557`).
- Validators with no transform: `.datetime()` (19 in agentApiContract), `.uuid()`, `.regex`, `.refine` /
  `.superRefine` (`agentApiContract.ts:349,450,466,506,551,1169`; `agentApiMessageContract.ts:141`),
  `discriminatedUnion` (4 in agentApiContract, plus actionCards and agentApiMessageContract), `z.record`
  (11), `z.unknown()` (10).

### 2.4 Recommended Rust mapping

- **Passthrough objects** become `struct { known fields…, #[serde(flatten)] extra: serde_json::Map<String, Value> }`
  and re-serialize `extra`. Enable `serde_json/preserve_order` so key order survives.
- **Plain `z.object`** (strip) becomes a struct with no `deny_unknown_fields` and no extra map. Unknown keys
  are dropped, which matches zod.
- **`.strict()` / `strictObject`** becomes `#[serde(deny_unknown_fields)]`. `deny_unknown_fields` does not
  combine with `flatten`, so avoid flattening inside strict types.
- **Transforms:** give the "validate + normalize" step its own function (for example `fn normalize(&mut self)
  -> Result<(), ContractError>`) that trims, applies defaults, and checks lengths, regexes and datetimes. Call
  it before sending and after receiving. Keep it in raft-shared next to the struct.
- **Optional vs nullable:** zod distinguishes `.optional()` (absent) from `.nullable()` (null). Use
  `Option<T>` + `skip_serializing_if = "Option::is_none"` for optional, and `Option<T>` without skip (or
  `Option<Option<T>>` where both occur) for nullable.
- **Agent-visible zod messages** (`action prepare`, §2.1): the Rust validator for `actionCardActionSchema`
  (`actionCards.ts:195-204`, 8-way discriminated union, no custom messages) must reproduce **zod v4's default
  English issue messages and paths** (for example `Invalid input: expected string, received number`,
  `Too small: expected string to have >=1 characters`, the discriminator-mismatch text). Capture golden
  outputs by running zod 4.3.6 against fixtures; do not guess them.
- **Daemon-api contract rejection diagnostics** (`daemonApiRawClient.ts:252-360`) are built from
  zod-internal issue codes (`unrecognized_keys`, `invalid_type` + `expected`/`received`, `invalid_value`, and
  the v3-era `invalid_literal` / `invalid_enum_value`) and printed by the CLI as
  `… (cause=…; path=…; expected_kind=…; actual_kind=…)` (`cli/src/daemonApiPath.ts:37-40`). The Rust
  validator must compute the same `{cause, path, expected_kind, actual_kind}` for the *first* failing field.
  Paths are sanitized: dynamic keys become `<dynamic-key>`, unknown keys `<unknown-key>`, the root `<root>`
  (`:230-250`). This API has only 5 small routes, so hand-writing it is fine.

---

## 3. Generated code and the route tables

### 3.1 What is generated

| artifact | generator | content | used by our 3 packages? |
|---|---|---|---|
| `shared/src/generated/agentApiRoutes.ts` (1,621 lines) | `shared/scripts/generate-agent-api-routes.ts` (17 lines): `JSON.stringify(buildAgentApiRouteManifest(), null, 2)` | `AGENT_API_ROUTE_MANIFEST`, 77 entries of **metadata only** | **No** (only `agentApiContract.test.ts`). The runtime table is the `agentApiContract` object itself |
| `shared/openapi/openapi.json` (938 lines) + `src/generated/openapi.ts` (481 lines) | `scripts/generate-openapi.ts` → `openapi-artifacts.ts` (zod-openapi 6.0.0 + openapi-typescript 7.13.0) from the registry in `src/openApiContract.ts` | **Only the 4 attachment-upload operations**: `GET /api/attachments/upload-capabilities`, `POST /api/attachments/upload-sessions`, `POST …/{uploadId}/complete`, `GET|DELETE …/{uploadId}` (see `openapi/README.md`: "P1 … not mounted") | No (web only: `web/src/utils/directAttachmentUpload.ts`) |
| `src/generated/runtimeProviderDisplayNames.ts` (27 lines) | `scripts/generate-runtime-provider-display-names.ts` from `shared/runtime-provider-display-names.json` | provider id → display name | yes (daemon `formatRuntimeProviderModelLabel` path) |
| `src/piBuiltinModels.generated.ts` | (external pi catalog) | pi provider/model tables | only the 18-line API-key-env map (§1.2) |

**Can we generate Rust from openapi.json?** Not usefully. It covers 4 of the 77 agent-api routes, and under
a different mount (`/api/attachments/*` rather than `/internal/agent-api/attachment-upload-sessions*`). The
agent-api request schemas do reuse the same zod objects (`createAttachmentUploadSessionRequestSchema` from
`attachmentUploadContract.ts`, strictObject).

**Can we generate from the route table?** Partly:
- The route **metadata** (key, method, path, fullPath, client resource/method, capability, which of
  params/query/body exist, json vs binary) can be derived mechanically. Parse `AGENT_API_ROUTE_MANIFEST` (it
  is JSON after the `export const … =` prefix) from a `build.rs`, or check in a generated `routes.rs`, plus a
  test that diffs it against the TS manifest.
- **Schemas:** the best option is a one-off TS script run in upstream that iterates `agentApiContract` and
  calls zod v4's `z.toJSONSchema(schema, { io: "input" | "output" })` per request/response. The output feeds
  `typify` or a hand-review pass. JSON Schema will **not** encode `.trim()`, `.default()` side effects,
  `superRefine` logic, or passthrough vs strip exactly (`additionalProperties` is only a hint), so the
  normalize/validate layer from §2.4 stays hand-written. Given ~2.6k lines of schema, generate the struct
  skeletons, then hand-audit.

### 3.2 Agent API route table shape

`AgentApiContractRoute` (`agentApiContract.ts:1686-1703`):
`{ key, method: "GET"|"POST"|"PATCH"|"DELETE", path, fullPath, client: {resource, method}, capability,
description, request: { params?: ZodType; query?: ZodType; body?: ZodType }, response: {kind?: "json", body: ZodType} | {kind: "binary"} }`.
`route()` (`:1705-1710`) sets `fullPath = AGENT_API_BASE_PATH + path`, where `AGENT_API_BASE_PATH =
"/internal/agent-api"` (`agentApiPaths.ts`).

There are 77 routes: 76 JSON, 1 binary (`attachmentDownload GET /attachments/:attachmentId`). 12 routes
have `:param` segments (`:msgId`, `:channelId`, `:artifactId`, `:reminderId`, `:appId`, `:uploadId`,
`:attachmentId`). Capabilities used: `send, read, knowledge, mcp, reactions, channels, server, mentions,
tasks`. Every route and its request flags:

```
POST   /feedback-locators                      feedbackLocatorIngest     body
GET    /feedback-locators                      feedbackLocatorList       query
GET    /events                                 events                    query
GET    /history                                historyRead               query
GET    /knowledge                              knowledgeGet              query
GET    /knowledge/search                       knowledgeSearch           query
GET    /wiki/manifest                          wikiManifestGet           -
GET    /wiki/artifacts/:artifactId             wikiArtifactRead          params
POST   /wiki/publish                           wikiManifestPublish       body
GET    /mcp/tools                              managedMcpTools           -
POST   /mcp/call                               managedMcpCall            body
POST   /send                                   messageSend               body
POST   /v2/send                                messageSendV2             body
GET    /messages/:msgId/resolve                messageResolve            params
GET    /search                                 messageSearch             query
POST   /messages/:msgId/reactions              messageReactionAdd        params+body
DELETE /messages/:msgId/reactions              messageReactionRemove     params+body
POST   /channels/:channelId/join               channelJoin               params
POST   /channels/:channelId/leave              channelLeave              params
POST   /channels/:channelId/mute               channelMute               params+body
POST   /channels/:channelId/unmute             channelUnmute             params
POST   /channels/archive | /channels/unarchive channelArchive/Unarchive  body
GET    /channel-members                        channelMembers            query
POST   /resolve-channel                        resolveChannel            body
POST   /threads/unfollow                       threadUnfollow            body
GET    /server  | PATCH /server                serverInfo / serverUpdate - / body
GET    /mention-actions/pending                mentionActionsPending     query
POST   /mention-actions/execute                mentionActionsExecute     body
POST   /tasks/{claim,unclaim,assign,update-status,resource-receipt,delete,convert,amend}  body
GET    /tasks  | POST /tasks                   taskList / taskCreate     query / body
GET    /tasks/history                          taskHistory               query
POST   /migrations | GET /migrations/current | POST /migrations/{ready,arrived}
GET    /reminders | POST /reminders            reminderList / reminderCreate
DELETE /reminders/:reminderId | POST …/snooze | PATCH /reminders/:reminderId | GET …/log
POST   /app-sources/ack                        appSourceAck              body
GET|PATCH /apps/:appId/config                  appConfigGet/Patch        params(+body)
GET    /profile | POST /profile | POST /profile/avatar (multipart)
GET    /integrations | GET /integrations/marketplace | POST /integrations/login
POST   /integrations/app/{prepare,rotate-secret,transfer-owner,update,manage,logo(multipart)}
GET    /integrations/app | GET /integrations/app/status
POST   /prepare-action                         actionPrepare             body
POST   /upload (multipart)                     attachmentUpload          -
GET    /attachment-upload-capabilities
POST   /attachment-upload-sessions | POST …/:uploadId/complete | DELETE|GET …/:uploadId
GET    /attachments/:attachmentId (binary)     attachmentDownload        params
GET    /attachments/:attachmentId/comments     attachmentCommentsList    params+query
```

Type maps: `AgentApiRequestParamsByRoute` (`:2827`), `…QueryByRoute` (`:2907`), `…BodyByRoute` (`:2987`),
and `AgentApiResponseByRoute` (`:3067-3145`). Absent parts are `never`. Compile-time asserts at
`:3147-3158` keep the maps exhaustive.

### 3.3 Client logic

**`buildAgentApiRawRoutePath`** (`agentApiRawClient.ts:208-231`):
1. Parse the params via zod. Replace `/:([A-Za-z][A-Za-z0-9_]*)/g` with
   `encodeURIComponent(String(parsed[key]))`. A missing value yields a `missing_path_param` failure.
2. The query is zod-parsed, then each entry goes into `URLSearchParams`: `undefined`/`null` are skipped,
   arrays append each item, everything else is `String(v)`. The suffix is `?${qs}` only when `size>0`.
3. Result: `(pathPrefix ?? "/internal/agent-api") + path + suffix`.

**`requestAgentApiRawRoute`** (`:233-304`): build the path, zod-parse the body, call
`transport.request({routeKey, method, path, body})`. A thrown transport yields `transport_error`. `!ok` yields
`http_error` carrying `status, errorCode, suggestedNextAction, proxy, response`. For binary routes the data
must be a `Uint8Array`. `data === null` yields `empty_response`. Otherwise `parseAgentApiResponse` runs, and a
failure there yields `response_contract_mismatch`. The error reasons are `missing_route | missing_path_param
| request_contract_mismatch | transport_error | http_error | empty_response | response_contract_mismatch`.
Message strings are fixed, for example ``Agent API ${key} body did not match the shared contract``.

**`createAgentApiRawClient`** (`:336-357`) builds `client[resource][method](...args)`. Args are positional in
the order params, query, body, and only the parts that exist (`requestOptionsFromMethodArgs`, `:306-334`).

**`createAgentApiClient`** (`agentApiClient.ts:272-298`) wraps the raw client and maps each failure to
`{kind:"transport"|"http"|"validation"}` (`errorFromRawFailure`, `:209-238`; `agentApiClientResultFromRaw`,
`:240-258`), and adds `.request(routeKey, {params,query,body})`. The optional fetch transport
(`createAgentApiFetchTransport`, `:174-207`) is **unused by the CLI**. The CLI passes its own transport
(`cli/src/agentApiPath.ts:70-98`) over its `ApiClient.request/requestBinary`. Legacy per-agent paths use
`buildLegacyAgentApiPath(agentId, routePath) = "/internal/agent/" + encodeURIComponent(agentId) + routePath`
(`agentApiContract.ts:2541-2543`), for example `buildContractAgentPathForRoute` (`cli/src/agentApiPath.ts:36-49`).
Multipart routes (`attachmentUpload`, `profileAvatarUpdate`, `integrationAppLogoUpdate`) bypass body
validation and go through `client.requestMultipart`.

For fetch-transport parity, if needed: non-OK error text comes from `data.error`, then `statusText`, then
`HTTP ${status}`. `errorCode` comes from `data.errorCode`, then `data.code`. Bodies are
`JSON.parse(text)`; an empty or whitespace-only body becomes `null`, and unparseable text is returned as the
raw string (`agentApiClient.ts:138-172`).

**Daemon API** (CLI → local daemon; `daemonApiContract.ts:228-274`, base also `"/internal/agent-api"`):
`runtimeVersion GET /runtime-version`, `inboxCheck GET /inbox`, `inboxAck POST /inbox/ack`,
`wakeHintsFetch GET /wake-hints`, `activityForward POST /activity`. Methods are only `GET|POST`. There are no
path params (`buildDaemonApiRawRoutePath`, `daemonApiRawClient.ts:415-427`). `createDaemonApiClient`
(`daemonApiClient.ts:262`) mirrors the agent client and adds `contractRejection` diagnostics (§2.4). The
daemon side that *serves* these routes does not import `daemonApiContract`.

Rust encoding notes: implement `encodeURIComponent` as percent-encoding of everything except
`A-Za-z0-9-_.!~*'()`. `URLSearchParams` uses WHATWG form-urlencoding (space becomes `+`); the
`form_urlencoded` crate matches it.

---

## 4. Clock helpers

`shared/src/clock.ts` (28 lines) is a set of **thin, non-injectable wrappers**. There is no global clock
object and no setter:

```1:27:upstream/raft-source/packages/shared/src/clock.ts
export function currentTimeMs(): number { return Date.now(); }
export function currentDate(): Date { return new Date(currentTimeMs()); }
export function currentRandomUnit(): number { return Math.random(); }
export function setClockInterval(fn, ms) { return setInterval(fn, ms); }
export function clearClockInterval(interval) { clearInterval(interval); }
export function setClockTimeout(fn, ms) { return setTimeout(fn, ms); }
export function clearClockTimeout(timeout) { clearTimeout(timeout); }
```

They exist as a lint-able seam ("without ambient Date.now in daemon files", `agentInboxApp.ts:154`). Tests
control time with vitest fake timers or node mock timers, which patch the globals these wrappers read. The
only hit in our packages is `daemon/src/agentProcessManager.codex.test.ts:7684`
(`vi.useFakeTimers({ toFake: ["setTimeout"] })`). Real injection happens per component instead:
`LocalRotatingTraceSink.nowMsProvider`, `BasicTracer.clock`,
`createRuntimeAccountUsageCollector({now})`, and similar.

Usage: 72 call sites of `currentTimeMs()`/`currentDate()`/`setClockTimeout()` across the three packages,
next to 146 direct `Date.now()`/`new Date()`/`setTimeout(` calls. The wrappers are not used consistently.

Rust recommendation: a `raft_shared::clock` module with `now_ms()`, `now()` (`SystemTime`/`chrono`), and
`tokio::time::sleep`/`timeout`. For tests, prefer `tokio::time::pause()` for timers plus an explicit
`Clock` trait where tests need wall-clock control. A global mutable clock is not required for parity.

---

## 5. Tracing and trace-client

### 5.1 shared `tracing/index.ts` (626 lines; 567 needed)

- Types: `TraceSurface = server|daemon|web|computer`, `TraceSpanKind`,
  `TraceStatus = unset|ok|error|cancelled`, `TraceContext {traceId, spanId, parentSpanId|null, traceFlags}`,
  `CompletedTraceSpan`, `TraceEvent {name, timeMs, attrs?}`, `TraceSink {record, recordEvent?, recordSpanFact?}`,
  `Tracer {startSpan(name, {parent?, surface, kind?, attrs?, startTimeMs?})}`,
  `ActiveSpan {context, addEvent, end(status="ok", {attrs})}` (`:1-80`).
- **`formatTraceparent(ctx)`** (`:124-127`): asserts the context is valid, then emits
  `"00-{traceId}-{spanId}-{flags}"`.
- **`parseTraceparent(v)`** (`:129-144`): regex `^([0-9a-f]{2})-([0-9a-f]{32})-([0-9a-f]{16})-([0-9a-f]{2})$`,
  lowercase only. The version must be `00`, and all-zero trace or span IDs are rejected. On success it returns
  `parentSpanId: null`; any failure returns `null`.
- **`noopTracer`** (`:171-189`) still mints a *valid random* context (it inherits the parent's traceId), so
  traceparent propagation works even when tracing is off.
- **`TraceScope {resource, request, actor}`** (`:262-266`) is projected by `projectTraceScopeAttrs`
  (`:268-298`) into snake_case attrs such as `daemon_version`, `daemon_version_present`, `machine_id`,
  `machine_id_present`. String values are trimmed and empty ones dropped; the `*_present` booleans are always
  emitted.
- **`createTraceScopeTracer(tracer, scope, {spanAttrContracts?, scopeAttrPrecedence?})`** (`:314-319`) stacks
  two wrappers. `ScopedTracer` (`:330-396`) merges scope attrs into `startSpan` and `end` attrs (scope wins
  by default) and strips inherited scope keys from event attrs. `SpanAttrContractTracer` (`:398-451`)
  allow-lists span, event, and end attrs per span name (`TraceSpanAttrContracts`).
- **`BasicTracer`** (`:460-496`) + `RecordingActiveSpan` (`:498-582`): the clock is injectable. IDs come from
  crypto random hex and must be non-zero. `addEvent` calls `sink.recordEvent?`. `end` computes
  `durationMs = max(0, end-start)`, calls `sink.recordSpanFact?` and then `sink.record`, and ignores repeat
  calls. `mergeAttrs` is `{...base, ...extra}`.

Daemon use: `core.ts` imports `createTraceScopeTracer`, `formatTraceparent`, `parseTraceparent`,
`noopTracer`, `TraceScope`, `TraceSpanAttrContracts`, `TraceStatus`. `agentCredentialProxy.ts` and
`agentProcessManager.ts` format and parse `traceparent` headers.

### 5.2 `trace-client` package (509 non-test src lines)

Files: `src/index.ts` (23), `traceClient.ts` (202), `localTraceSink.ts` (227), `traceJitter.ts` (57); tests
`localTraceSink.test.ts` (628), `traceClient.test.ts` (144), `traceJitter.test.ts` (72); `README.md` (118).
The package declares `@botiverse/raft-shared` only as a **devDependency**, yet imports `BasicTracer` from it
at runtime (`traceClient.ts:1-12`). In Rust, `raft-trace-client` depends on `raft-shared`.

API surface used:
- daemon `core.ts:74-79`: `LocalRotatingTraceSink`, `computeTraceJitter`, `createTraceClient`, `NO_JITTER`,
  `TraceJitter`. daemon `traceBundleUpload.ts:7`: `bucketDelayMs`, `computeTraceJitter`, `NO_JITTER`,
  `TraceJitter`.
- computer `lib/computerTracer.ts:1`: `createTraceClient`, `LocalRotatingTraceSink`.

**`createTraceClient({source, sinks})`** (`traceClient.ts:193-197`) builds
`new SourceForcingTracer(new BasicTracer({sink: new MultiSink(sinks)}), source)`. `source` is
`"daemon" | "computer.cli" | "computer.menu-bar"` and is force-set as the `source` span attr: caller attrs
are overridden at start, and `source` is stripped from end attrs (`:132-181`). `MultiSink` isolates sink
failures but forwards only `record` (it drops `recordEvent`/`recordSpanFact`, `:104-122`).

**`LocalRotatingTraceSink`** (`localTraceSink.ts`). This is what gets written to disk:
- Directory `<machineDir>/traces/` (mode `0o700`). File name
  `daemon-trace-${new Date(now).toISOString().replace(/[:.]/g,"-")}-${pid}-${seq:04}.jsonl`. The prefix is
  `daemon-trace-` even for computer. New files are created with mode `0o600`.
- Rotation happens when `size + nextLineBytes > maxFileBytes` (default 5 MiB, min 1024) or when
  `now - openedAt >= maxFileAgeMs + jitter` (default 5 min, min 1 s). After opening a new file it prunes to
  `maxFiles` (default 8) by lexical sort. All errors are swallowed.
- One JSON line per completed span (`toLocalTraceRecord`, `:131-148`), with key order:
  `{type:"span", schema_version:1, trace_id, span_id, parent_span_id, name, surface, kind, status,
  start_time(ISO), end_time(ISO), duration_ms, attrs, events:[{name, time(ISO), attrs}]}`. When there are no
  attrs, `attrs` is `undefined` and therefore **omitted** from the JSON.
- **Attr sanitization** (`:158-219`) must be ported exactly:
  - Drop `null`, `undefined` and `""` values.
  - Keep the allow-listed ID keys (`DIAGNOSTIC_ID_ATTRS`, `:9-28`).
  - `DIAGNOSTIC_ERROR_ATTRS` (`:30-39`): `original_message` is redacted with the patterns
    `sk_(agent|machine|computer)_…`→`sk_[redacted]`, `sap_…`→`sap_[redacted]`, `https?://\S+`→`[url]`,
    then whitespace is collapsed, the value is trimmed, and it is truncated to 240 (237 + `...`).
  - Other keys are normalized to snake_case, then dropped if they match the secret regex, kept if they match
    the suffix allow-list (`count|present|kind|mode|source|outcome|reason|class|status|bucket|ms|code|truncated`),
    dropped if they end in `_id`, and dropped if they match the content regex
    (`prompt|content|text|message|body|request|response|command|argv|env|cwd|path|file|error|tool_args|…`).
  - Values: arrays become `{items_count:n}`, objects become `{object_present:true}`.
- Env gates, set by callers: daemon `SLOCK_DAEMON_LOCAL_TRACE=0`,
  `SLOCK_DAEMON_TRACE_MAX_FILE_BYTES|_MAX_FILE_AGE_MS|_MAX_FILES`, `SLOCK_DAEMON_TRACE_JITTER_DISABLED=1`
  (`daemon/src/core.ts:1645-1673`); computer `RAFT_COMPUTER_LOCAL_TRACE=0`, which writes under
  `computerDir(slockHome)` (`computer/src/lib/computerTracer.ts:13-23`).

**`computeTraceJitter(lockId)`** (`traceJitter.ts:31-38`): `sha256(lockId)` →
`readUInt32BE(0) % 30000`, `readUInt32BE(4) % 60000`, `readUInt32BE(8) % 60000`. `bucketDelayMs`
(`:48-57`) produces the labels `0-1s | 1-5s | 5-15s | 15-30s | 30-60s | 60s-5m | 5-10m | 10m+`.

**HTTP upload is not in trace-client.** It lives in the daemon (`daemon/src/traceBundleUpload.ts`, 267 lines;
`directUploadCapability.ts`, 191 lines):
1. `POST {serverUrl}/internal/machine/scope-attestation` with `Authorization: Bearer <apiKey>` and body
   `{scope:"daemon-trace-bundle:create", metadata?}`.
2. `POST {workerUrl}/api/trace-bundles` with `{bundleId(uuid), bundleSha256, bundleSizeBytes, …,
   attestation}`.
3. Upload the gzip of the `.jsonl` file (`application/x-ndjson`) to the returned signed URL, using its method
   and headers.
4. Write `<stateDir>/<file>.uploaded.json` with `uploadedAt`.

The default worker is `https://slock-trace-upload.botiverse.dev` (`core.ts:146`), overridable with
`SLOCK_DAEMON_TRACE_UPLOAD_URL` and disabled with `SLOCK_DAEMON_TRACE_UPLOAD_DISABLED=1`. The span name is
`daemon.bundle.upload`.

---

## 6. Agent-visible formatting helpers (must be byte-exact)

| helper | source | consumers | notes |
|---|---|---|---|
| `formatAgentInboxSnapshot(rows)` | `agentInbox.ts:90-111` | cli `commands/inbox/_format.ts`, daemon `agentInboxProjection.ts` | `"Inbox: empty"`. The header switches to `"Inbox: N pending target(s) · M target(s) with suppressed items"` when there are suppressed-only rows. Each row is target, details, blank line. `.trimEnd()` |
| `formatAgentInboxDelta(rows,{totalPendingMessages})` | `agentInbox.ts:113-126` | daemon | `"Inbox update: …"` variants; the row is `target + "  " + details` |
| row details (private) | `agentInbox.ts:128-137` | – | `pending: N message(s)` · `K suppressed (not delivered)` · `first msg=<8>` · `latest sender @name` · `latest msg=<8>` · flags · `attention_hint=<JSON.stringify of 8 fixed keys>` joined with `" · "` (U+00B7) |
| flag labels | `agentInbox.ts:139-156` | – | `mention` → `you were mentioned`; `non_member_mention` → the guidance string; unknown flags → ``unknown inbox flag: ${JSON.stringify(bounded40…)}`` (U+2026) |
| `formatAgentReplyAffordance` / `…Suffix` | `agentInbox.ts:3-4, 23-30` | cli `mention/_format.ts`, `message/_format.ts`; daemon | fixed `[Raft notice: You were notified as a non-member, …]`. The suffix adds a leading `\n` |
| `formatAgentInboxAppItems` / `formatAgentInboxFullSnapshot` | `agentInboxApp.ts:106-148` | cli | `app=… · class=… · item=<8> · retention=… · sourceRef=kind:id[:rev] · action=… [· title=…][· summary=…]`. `"App items: N"` header. With message rows equal to `"Inbox: empty"`, only the app part is shown |
| `shortIdFromSourceRef`, `sourceRefIdentityKey` | `agentInboxApp.ts:96-104` | daemon | `id.slice(0,8)`; `kind\0id\0rev` |
| `formatUtcTimestamp(v)` | `utcTimestamp.ts:7-13` | cli message/task formats; daemon `historyFormatting.ts`, `logger.ts`, `agentRuntimeInput.ts` | `toISOString().slice(0,19).replace("T"," ")+"Z"`. Invalid dates pass through as the input string. **JS `new Date(string)` leniency** (non-ISO formats; a date-time without offset is *local* time; a date-only value is UTC) must be emulated |
| `getRuntimeDisplayName(id)` / `isRuntimeDeprecated` | `index.ts:1788-1792`, `1756-1758`; table `RUNTIMES` `index.ts:1703-1722` | cli `profile/_format.ts`; daemon `runtimeErrorDiagnostics.ts` | external-agent runtime → `EXTERNAL_AGENT_RUNTIME_DISPLAY_NAME`, otherwise the table value, otherwise the id. Keep the `builtin`/`kimi-sdk`/`pi` rows even though those runtimes are dropped (agent profiles still reference them) |
| `formatRuntimeProviderModelLabel` | `runtimeProviderDisplay.ts:11-27` (+`:38-72`, generated names) | daemon `drivers/opencode.ts` | model label humanization |
| `RaftTargetString` (type) + `parseRaftRefTarget` / `formatRaftRefTarget` | `raftRefs.ts:91-103`, `128-210` | cli `message/_format.ts` | the target grammar `@u`, `#c`, `#c:thr`, `dm:@p`, `dm:@p:thr`, `task #n`, `… msg=<id>`, `computer:<uuid>`, `app:<a.b>` (`:1-11`, `:105-126`) |
| `structuredRaftMentionStillAppears(src, handle)` | `raftRefs.ts:366-392` (+`markdownCodeSpans` `:229-268`, `replaceOutsideMarkdownCode` `:270-320`) | cli `message/send.ts` | skips fenced and inline code, escaped `\<@x>`, and `[label](<computer:…/app:…>)` links; regex `@handle(?![\p{L}\p{N}_-])` |
| `joinRaftChannelByTarget(client, {target})` | `agentApiChannelJoin.ts:85-169` | cli `channel/join.ts` | calls `server.info()`, finds the channel by name, then calls `channels.join`. Fixed messages: `"Target must be a regular channel in the form '#channel-name'. DMs and thread targets are not supported."` and ``Channel not found: ${target}``. States `already_joined` / `joined` |
| `renderThirdPartyInertJson(obj)` | `thirdPartyInertRenderer.ts:116-118` (+`:35-108`, `extractRaftRefTargets` `raftRefs.ts:524-543`) | cli `message/_format.ts`; daemon `agentRuntimeInput.ts` | `JSON.stringify(v,null,2)`, then escape `</?(result\|preview\|match)…>` and `<omit/>` as `&lt;…&gt;`, then rewrite raft refs to neutral labels (`user:x`, `channel:x`, `dm:user:x`, `task:n`, …) |
| `formatProducerFactLineageBracket` | `producerFactLineage.ts:35-38` | daemon `agentRuntimeInput.ts` | ` [producerFactId=…]` bracket |
| `getToolActivityLabel`, `summarizeToolInput`, `resolveToolSemantic`, `normalizeToolDisplayInvocation` | `toolDisplay.ts:181,468-533` (496-line closure) | daemon `agentProcessManager.ts` | activity text sent to the server and UI |
| `TASK_CLAIM_REASON_ALREADY_CLAIMED_BY_YOU` | `index.ts:2901-2918` | cli task claim/format | constant reason string |
| `AGENT_API_ATTACHMENT_DOWNLOAD_UNAVAILABLE_MESSAGE` / `_NEXT_ACTION` | `agentApiContract.ts:70-83` | cli `attachment/view.ts` | fixed strings |
| `AGENT_LOGIN_INTEGRATION_INVENTORY_PROJECTION` | `capabilityInventories.ts:100-105` | cli integration formats | projection table |

**Byte-exactness hazards (JS vs Rust):**
- **String lengths and slices are in UTF-16 code units.** `slice(0,8)`, `length > 40`, the 4,000-char clamp in
  `thirdPartyInertRenderer.ts:35-38`, the 80-char labels, and regex match offsets used as `start`/`end`
  indexes in `raftRefs.ts` all count UTF-16. Implement with `encode_utf16` indexing or a helper; a naive
  `&s[..8]` can panic or disagree.
- **Regex semantics.** JS `\w`, `\d` and `\b` are ASCII even with `u`. Rust `regex` defaults them to Unicode,
  so use `(?-u:\w)` or explicit classes. Note that JS `\w` under the `iu` flags also matches U+017F and
  U+212A (case folding); `createRaftDmRefRegex` and friends use `giu`. Lookahead `(?![…])`
  (`raftRefs.ts:29`) is unsupported by `regex`: use `fancy-regex` or a manual boundary check. `\s` differs
  slightly (JS includes U+FEFF).
- **`JSON.stringify`**: key order is insertion order, but integer-like keys are hoisted and sorted first.
  Numbers use shortest round-trip form with no `.0` (serde_json prints `1.0` for f64 `1.0`) and exponent form
  at ≥1e21. Use a JS-compatible serializer (for example format f64 via `ryu_js`), or keep integral values
  typed as integers. This affects the `attention_hint=` JSON, `renderThirdPartyInertJson` pretty output
  (2-space indent; `[]`/`{}` for empties, which matches serde's `PrettyFormatter::with_indent(b"  ")`), and
  oar `usedRatio` values like `1`.
- `toISOString()` always has milliseconds (`YYYY-MM-DDTHH:mm:ss.sssZ`). Use
  `chrono::SecondsFormat::Millis, true`.

---

## 7. `@botiverse/oar` v0.0.7

### 7.1 Structure (`oar/src`, 5,725 lines total incl. README)

- `contracts/` (413): `runtime.ts` (15) `Runtime {id, session, installation?, accountUsage?}`;
  `installation.ts` (33); `account-usage.ts` (40); `session.ts` (190); `model-catalog.ts`;
  `provider-auth.ts`.
- `runtimes/{claude,codex,grok,kimi,pi}/`: session drivers, projections, account-usage readers, installation
  probes.
- `shared/`: `executable/` (process spawn, `which`, version), `acp/` (ACP client, session, terminal),
  `installation.ts`, `json.ts`, `instant.ts`, `session-kernel.ts`, and others.
- `observe/`, `voyage.ts`, `registry.ts`, `index.ts` (107): re-exports; `runtimes` registry.
- `package.json` dependencies: `@agentclientprotocol/sdk ^1.4.0`, `cross-spawn ^7`, `which ^2`; optional
  `@earendil-works/pi-coding-agent`. Requires Node ≥ 24.

### 7.2 What the daemon uses

Only `daemon/src/runtimeAccountUsage/collector.ts` (106 lines) and `oarAdapter.ts` (149) use oar:
`claudeRuntime`, `codexRuntime`, `kimiRuntime`, `grokRuntime` and types `Runtime`, `AccountUsageSnapshot`.
Only `runtime.installation()` and `runtime.accountUsage(installation, {timeoutMs: 20000})` are called
(`collector.ts:57-73`). **Sessions are never used**, so `contracts/session.ts`, `shared/acp/session.ts`,
`terminal.ts`, `projection.ts`, `observe/*` and pi are all out of scope.

The oar contract is `AccountUsageSnapshot = {kind:"available", plan?, email?, rateLimited, windows:[{label,
usedRatio 0..1, resetsAt? ISO}]} | {kind:"reauth_required"} | {kind:"unsupported"}`
(`contracts/account-usage.ts:20-31`). Installation is `{kind:"available", via:"executable", command,
version?} | {kind:"not_found"} | {kind:"unsupported", reason}`.

**Common installation probe** `executableInstallation(envVar, cmd, fallbacks, readiness?, opts)`
(`shared/installation.ts:37-83`): candidates are `$envVar` exclusively, otherwise `[cmd, …fallbacks]`. A
candidate with a path separator must exist; a bare name is resolved with `which.sync`. If readiness args are
given, each candidate is run with them until one succeeds; a spawn failure throws. The version is the first
line of `--version` stdout. `runExecutable` is `execFile` with a 5 s default timeout, 2 MiB maxBuffer, and
shell only for Windows `.cmd`/`.bat` (`shared/executable/run.ts`).

| runtime | installation | what `accountUsage` does |
|---|---|---|
| **claude** (`runtimes/claude/account-usage.ts`, 235) | `OAR_CLAUDE_BIN` / `claude` | 1. **Spawns** `claude auth status --json` (env `CLAUDECODE` unset). Failure or `loggedIn:false` → `reauth_required`; an `apiKeySource` value or an `authMethod` other than `claude.ai` → `unsupported`. 2. **Reads the local credential** `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` → `claudeAiOauth.accessToken`, which must have the `user:profile` scope. On macOS the fallback is `security find-generic-password -w -s "Claude Code-credentials"`. 3. **HTTP** `GET https://api.anthropic.com/api/oauth/usage` with headers `Authorization: Bearer`, `anthropic-beta: oauth-2025-04-20`, `User-Agent: claude-code/<version>`; timeout ≤ 5 s. 401/403 → `reauth_required`. `limits[]` map to windows: `session` → "Current session", `weekly_all` → "Current week (all models)", `weekly_scoped` → "Current week (<model>)"; `usedRatio = Number((percent/100).toFixed(6))`; `rateLimited` if severity is `critical` or percent ≥ 100. Plan comes from `subscriptionType` |
| **codex** (`account-usage.ts` 261 + `app-server-client.ts` 96) | `OAR_CODEX_BIN` / `codex`, plus macOS bundles `/Applications/{ChatGPT,Codex}.app/Contents/Resources/codex` and the same under `~/Applications`; readiness `app-server --help` | **Spawns** `codex app-server --listen stdio://` and speaks newline-delimited JSON-RPC, **without a `"jsonrpc"` field**: `{id, method, params}`. Sequence: `initialize {clientInfo:{name:"oar",version:"0.0.0"}, capabilities:{experimentalApi:true}}`, notify `initialized`, `account/read` (optional), `account/rateLimits/read`. API-key, Bedrock, or `requiresOpenaiAuth:false` → `unsupported`. An error matching `/authentication required/i` → `reauth_required`; `/method not found\|not supported/i` → `unsupported`. Buckets merge `rateLimits` + `rateLimitsByLimitId` (dedupe by limitId). Window labels are `"<limitName\|Codex> · <N week(s)/day(s)/hour(s)/minutes>"`. `resetsAt` is epoch s or ms. Timeout 8 s default (20 s from the daemon), then `kill`; the kill sends SIGTERM and closes stdin |
| **kimi** (`account-usage.ts` 298 + `auth-config.ts` 110 + `oauth-token.ts` 41) | `OAR_KIMI_BIN` / `kimi`, plus `$KIMI_INSTALL_DIR/bin/kimi`, `~/.kimi-code/bin/kimi`, `kimi-code`; readiness `acp --help` (30 s) | 1. **Spawns** `kimi provider list --json` and takes `providers["managed:kimi-code"]` (type `kimi`); baseUrl and oauthHost can be overridden by `KIMI_CODE_BASE_URL`, `KIMI_CODE_OAUTH_HOST`/`KIMI_OAUTH_HOST`. The OAuth key is `oauth/kimi-code`, or `oauth/kimi-code-env-<sha256(JSON{oauthHost,baseUrl})[:16]>`. Non-file storage → `unsupported`. 2. **Reads the local file** `${KIMI_CODE_HOME:-~/.kimi-code}/credentials/<name>.json` → `access_token` (never refreshed). 3. **HTTP**, in parallel: `GET {baseUrl}/usages` and `GET {baseUrl}/me` (default `https://api.kimi.com/coding/v1`). 401/403 → reauth; 404 → unsupported. Rows are `usage` (weekly) plus `limits[]`, with labels `Weekly limit`, `<n><m\|h\|d> limit` or the name, plus the booster wallet `Extra Usage monthly limit`. Deadline 30 s default |
| **grok** (`account-usage.ts` 197; needs `session.ts:5-34`, `shared/acp/process.ts` 126, `acp/errors.ts` 41) | `OAR_GROK_BIN` / `grok`, plus `$GROK_BIN_DIR/grok`, `$GROK_HOME/bin/grok`, `~/.grok/bin/grok`; readiness `agent stdio --help` | **Spawns** `grok agent --always-approve --no-leader stdio` and speaks **ACP** (JSON-RPC 2.0 over ndjson through `@agentclientprotocol/sdk`). Sequence: `initialize {protocolVersion: PROTOCOL_VERSION, clientCapabilities:{fs:{readTextFile:false,writeTextFile:false},terminal:false}, clientInfo, _meta: grokInitializeMeta}`; `authenticate {methodId}` if `_meta.defaultAuthMethodId` or `cached_token` is offered; `_x.ai/billing`; `_x.ai/auth/info` (optional, gives the email). RequestError −32601 → unsupported; −32000 or an auth-ish message → reauth. Projection: the `config.creditUsagePercent` value (or used/monthlyLimit cents) gives the included-usage window, plus an optional "Pay-as-you-go" window; plan comes from `subscription_tier`. No local credential files |

**The daemon projection** (`oarAdapter.ts`, 149) turns each snapshot into the raft wire
`RuntimeAccountUsageSnapshot` (`shared/runtimeAccountUsage.ts:159`, `protocolVersion 2`, providers
`claude|codex|kimi|grok`, healths `ok|rate_limited|reauth_required|unsupported|error`):
- `accountKey = sha256(provider + "\0" + slot)`, window id `w{i}_{sha256(label)[:12]}`, stale after 30 min.
- At most 12 windows; more than that makes the reading not representable.
- The email is masked by `maskRuntimeAccountEmail` (`runtimeAccountUsage.ts:34-81`).

### 7.3 Line counts of the daemon-used oar parts

contracts (account-usage, installation, runtime) 88 · shared (installation 83, executable/* 203, instant 7,
json 35) 328 · claude 238 · codex 380 · kimi 478 · grok 466 (account-usage 197 + installation 29 +
session.ts 73, of which only ~30 are needed + acp/process 126 + acp/errors 41) → **≈1,990 lines** (≈1,950 net).
The external ACP SDK is replaced by a minimal JSON-RPC 2.0 ndjson client in Rust, covering request/response,
error `code`/`message`, and per-request deadlines. Verify the ACP `PROTOCOL_VERSION` value (the SDK is not
installed locally) against `@agentclientprotocol/sdk@^1.4`; it is believed to be `1`.

### 7.4 Tests available (vitest, repo-root `oar/../../tests`)

Relevant: `tests/account-usage.test.ts` (298), `claude-account-usage-reader.test.ts` (107),
`codex-account-usage-reader.test.ts` (174), `kimi/kimi-account-usage-reader.test.ts` (182),
`kimi/kimi-account-usage.test.ts` (142), `grok/grok-account-usage-reader.test.ts` (98),
`installation.test.ts` (227), `process.test.ts` (89), `acp/acp-process.test.ts` (120), and the fixture
`fixtures/fake-acp-agent.mjs` (266). Total ≈1,700 lines, portable as projection golden tests plus fake-CLI
integration tests. Daemon-side tests: `runtimeAccountUsage/collector.test.ts` (151), `oarAdapter.test.ts`
(184), `oarKimi.test.ts` (130).

---

## 8. Cross-package cycles and direct `src/` imports

**Direct `@botiverse/raft-shared/src/...` imports:** all in the daemon, 12 import statements. These modules are
**not** re-exported from `index.ts`, so the Rust crate must make them `pub`:
- `src/appRuntimeTrace.js`: `agentAppInbox.ts:26`, `agentProcessManager.ts:41`, `core.ts:44`,
  `apps/cleaner/configReceiver.ts:6`, `apps/cleaner/runtime.ts:13`, `apps/reminder/runtime.ts:11`
- `src/appConfigTransport.js`: `apps/cleaner/configReceiver.ts:10` (`AppConfigWireSnapshot`,
  `normalizeAppConfigWireSnapshot`)
- `src/apps/cleaner/configProtocol.js`: `apps/cleaner/configReceiver.ts:15`, `definition.ts:18`,
  `runtime.ts:18`, `registry.manifest.ts:10`
- `src/apps/reminder/protocol.js`: `registry.manifest.ts:11`

The CLI and computer have none.

**Package-level edges:**
- `daemon → cli`: `daemon/src/core.ts:1255-1262` `runBundledRaftCli` does
  `await import("@botiverse/raft/dist/index.js")`, which runs the CLI in-process for `<exe> __cli`.
  `daemon/package.json` depends on `@botiverse/raft`.
- `computer → daemon`: `import("@botiverse/raft-daemon/core")` (`computer/src/service.ts:400`,
  `cli.ts:894`) plus the type `DaemonCoreOptions`.
- `trace-client → shared` (runtime import despite being a devDependency); `shared → sync-core` (not needed,
  §1).
- **No package cycle.** The graph is computer → daemon → cli → shared, with daemon and computer → trace-client
  → shared. In Rust: `raft-shared ← raft-trace-client`, `raft-cli ← raft-daemon-core ← raft-computer`.
  `oar` is a leaf used by daemon-core. The CLI must be a library crate with a `run(argv)` entry so daemon-core
  can embed it.

**Intra-shared cycles (types only, harmless in Rust):** `agentApiContract.ts:9` imports types
(`ProfileView`, `TaskResourceReceipt`, `TaskStatus`) from `./index.js`, while `index.ts` re-exports
`agentApiContract`. `appRuntimeTrace.ts:1` imports types from `./index.js`. `index.ts:692,697` inline-imports
`./appConfigTransport.js` types. Split `index.ts`'s inline type definitions (wire messages, `AgentConfig`,
profile views, runtimes) into Rust modules such as `wire`, `agent`, `runtime`, `profile`.

---

## Appendix: symbol → definition (264 rows)

Line = declaration start (including leading JSDoc) in `shared/src/`. Kind: const / fn / type / interface /
class / zod schema (a const whose name ends in `Schema`). "(subpath)" = imported via
`@botiverse/raft-shared/src/...`. The trace-client rows are the shared symbols trace-client itself imports.

| symbol | defined at | kind | used by |
|---|---|---|---|
| actionCardActionSchema | actionCards.ts:195 | zod schema | cli |
| validateActionCardAction | actionCards.ts:388 | fn | cli |
| joinRaftChannelByTarget | agentApiChannelJoin.ts:91 | fn | cli |
| agentApiClientResultFromRaw | agentApiClient.ts:240 | fn | cli |
| createAgentApiClient | agentApiClient.ts:272 | fn | cli |
| AgentApiClientFailure | agentApiClient.ts:62 | interface | cli |
| AgentApiClientResult | agentApiClient.ts:69 | type | cli |
| agentApiContract | agentApiContract.ts:1712 | const (route table of zod schemas) | cli |
| AgentApiRouteKey | agentApiContract.ts:2492 | type | cli |
| getAgentApiResponseKind | agentApiContract.ts:2516 | fn | cli |
| buildLegacyAgentApiPath | agentApiContract.ts:2541 | fn | cli |
| AgentApiTaskClaimBody | agentApiContract.ts:2569 | type | cli |
| AgentApiTaskListQuery | agentApiContract.ts:2570 | type | cli |
| AgentApiTaskCreateBody | agentApiContract.ts:2571 | type | cli |
| AgentApiTaskUnclaimBody | agentApiContract.ts:2572 | type | cli |
| AgentApiTaskAssignBody | agentApiContract.ts:2573 | type | cli |
| AgentApiTaskUpdateStatusBody | agentApiContract.ts:2574 | type | cli |
| AgentApiTaskResourceReceiptBody | agentApiContract.ts:2575 | type | cli |
| AgentApiTaskDeleteBody | agentApiContract.ts:2576 | type | cli |
| AgentApiTaskConvertBody | agentApiContract.ts:2577 | type | cli |
| AgentApiTaskAmendBody | agentApiContract.ts:2578 | type | cli |
| AgentApiAppConfigPatchBody | agentApiContract.ts:2593 | type | cli |
| AgentApiAppConfigResponse | agentApiContract.ts:2594 | type | cli |
| AgentApiTaskClaimSuccessResponse | agentApiContract.ts:2687 | interface | cli |
| AgentApiActionPrepareResponse | agentApiContract.ts:2785 | type | cli |
| AgentApiServerInfoResponse | agentApiContract.ts:2786 | type | cli |
| AgentApiIntegrationMarketplaceResponse | agentApiContract.ts:2799 | type | cli |
| AgentApiIntegrationAppPrepareResponse | agentApiContract.ts:2801 | type | cli |
| AgentApiIntegrationAppRotateSecretResponse | agentApiContract.ts:2803 | type | cli |
| AgentApiIntegrationAppTransferOwnerResponse | agentApiContract.ts:2805 | type | cli |
| AgentApiIntegrationAppUpdateResponse | agentApiContract.ts:2807 | type | cli |
| AgentApiIntegrationAppManageResponse | agentApiContract.ts:2809 | type | cli |
| AgentApiIntegrationAppLogoResponse | agentApiContract.ts:2810 | type | cli |
| AgentApiOwnedIntegrationApp | agentApiContract.ts:2811 | type | cli |
| AgentApiIntegrationAppListResponse | agentApiContract.ts:2812 | type | cli |
| AgentApiIntegrationAppStatusResponse | agentApiContract.ts:2813 | type | cli |
| AgentApiMigrationResponse | agentApiContract.ts:2820 | type | cli |
| AgentApiMigrationStatusResponse | agentApiContract.ts:2821 | type | cli |
| AgentApiRequestParamsByRoute | agentApiContract.ts:2827 | type | cli |
| AgentApiRequestQueryByRoute | agentApiContract.ts:2907 | type | cli |
| AgentApiRequestBodyByRoute | agentApiContract.ts:2987 | type | cli |
| AgentApiResponseByRoute | agentApiContract.ts:3067 | type | cli |
| parseAgentApiResponse | agentApiContract.ts:3160 | fn | cli |
| parseAgentApiAppSourceAckReject | agentApiContract.ts:3186 | fn | daemon |
| AGENT_API_ATTACHMENT_DOWNLOAD_UNAVAILABLE_MESSAGE | agentApiContract.ts:70 | const | cli |
| AGENT_API_ATTACHMENT_DOWNLOAD_UNAVAILABLE_NEXT_ACTION | agentApiContract.ts:77 | const | cli |
| AgentApiStructuredMention | agentApiMessageContract.ts:14 | interface | cli |
| agentApiStructuredMentionSchema | agentApiMessageContract.ts:20 | zod schema | cli |
| AgentApiSendV2Body | agentApiMessageContract.ts:221 | interface | cli |
| AgentApiHeldFreshnessResponse | agentApiMessageContract.ts:285 | interface | cli |
| AgentApiSendSentResponse | agentApiMessageContract.ts:318 | interface | cli |
| buildAgentApiRawRoutePath | agentApiRawClient.ts:208 | fn | cli |
| requestAgentApiRawRoute | agentApiRawClient.ts:233 | fn | cli |
| AgentApiRawTransport | agentApiRawClient.ts:41 | interface | cli |
| AgentApiRawFailure | agentApiRawClient.ts:52 | type | cli |
| AgentApiRawResult | agentApiRawClient.ts:65 | type | cli |
| formatAgentInboxDelta | agentInbox.ts:113 | fn | daemon |
| formatAgentReplyAffordance | agentInbox.ts:23 | fn | cli |
| formatAgentReplyAffordanceSuffix | agentInbox.ts:27 | fn | cli,daemon |
| AgentInboxFlag | agentInbox.ts:46 | type | daemon |
| AgentInboxTargetRow | agentInbox.ts:48 | type | cli,daemon |
| AGENT_INBOX_TARGET_ROW_KEYS | agentInbox.ts:74 | const | daemon |
| formatAgentInboxSnapshot | agentInbox.ts:90 | fn | cli,daemon |
| shortIdFromSourceRef | agentInboxApp.ts:101 | fn | daemon |
| formatAgentInboxAppItems | agentInboxApp.ts:120 | fn | cli |
| formatAgentInboxFullSnapshot | agentInboxApp.ts:131 | fn | cli |
| AgentInboxPrimaryAction | agentInboxApp.ts:17 | type | daemon |
| AgentInboxRetention | agentInboxApp.ts:29 | type | daemon |
| AgentInboxSourceRef | agentInboxApp.ts:36 | type | cli,daemon |
| AgentInboxAppItem | agentInboxApp.ts:46 | type | cli,daemon |
| AGENT_INBOX_PREVIEW_MAX_CHARS | agentInboxApp.ts:66 | const | daemon |
| AgentInboxItem | agentInboxApp.ts:76 | type | cli |
| sourceRefIdentityKey | agentInboxApp.ts:96 | fn | daemon |
| agentMigrationTransferSummarySchema | agentMigration.ts:29 | zod schema | daemon |
| AgentMigrationTransferSummary | agentMigration.ts:55 | type | daemon |
| AGENT_MIGRATION_SOURCE_WORKSPACE_ARCHIVE_CAPABILITY | agentMigrationResumable.ts:15 | const | daemon |
| AGENT_MIGRATION_DEFAULT_CHUNK_BYTES | agentMigrationResumable.ts:17 | const | daemon |
| AGENT_MIGRATION_MIN_CHUNK_BYTES | agentMigrationResumable.ts:18 | const | daemon |
| AGENT_MIGRATION_MAX_CHUNKS | agentMigrationResumable.ts:19 | const | daemon |
| AGENT_MIGRATION_MAX_CONTROL_MANIFEST_BYTES | agentMigrationResumable.ts:20 | const | daemon |
| AGENT_MIGRATION_MAX_ARCHIVE_ENTRIES | agentMigrationResumable.ts:21 | const | daemon |
| AGENT_MIGRATION_COMMIT_MARKER_PATH | agentMigrationResumable.ts:22 | const | daemon |
| AGENT_MIGRATION_BUNDLE_CONTENT_TYPE | agentMigrationResumable.ts:23 | const | daemon |
| AgentMigrationControlChunk | agentMigrationResumable.ts:26 | interface | daemon |
| AgentMigrationControlManifest | agentMigrationResumable.ts:33 | interface | daemon |
| AGENT_MIGRATION_RESUMABLE_PROTOCOL | agentMigrationResumable.ts:7 | const | daemon |
| AGENT_MIGRATION_CONTROL_SCHEMA_VERSION | agentMigrationResumable.ts:8 | const | daemon |
| AGENT_MIGRATION_RESUMABLE_CAPABILITIES | agentMigrationResumable.ts:9 | const | daemon |
| ApmFreshnessDecisionProducerInput | apmHeldFreshness.ts:1 | interface | daemon |
| projectApmHeldFreshnessEnvelope | apmHeldFreshness.ts:117 | fn | daemon |
| ApmFreshnessSideEffectAction | apmHeldFreshness.ts:13 | type | daemon |
| ApmFreshnessHeldDecision | apmHeldFreshness.ts:14 | type | daemon |
| projectApmHeldFreshnessActivity | apmHeldFreshness.ts:162 | fn | daemon |
| projectApmFreshnessDecisionTrace | apmHeldFreshness.ts:218 | fn | daemon |
| ApmHeldFreshnessEnvelopeBody | apmHeldFreshness.ts:50 | type | daemon |
| ApmHeldFreshnessEnvelopeProjection | apmHeldFreshness.ts:54 | interface | daemon |
| ApmHeldFreshnessActivityEntry | apmHeldFreshness.ts:62 | interface | daemon |
| ApmHeldFreshnessActivityProjection | apmHeldFreshness.ts:78 | interface | daemon |
| ApmFreshnessDecisionTraceProjection | apmHeldFreshness.ts:88 | interface | daemon |
| buildApmFreshnessDecisionProducerFactId | apmHeldFreshness.ts:96 | fn | daemon |
| AppConfigWireSnapshot (subpath) | appConfigTransport.ts:23 | type | daemon |
| normalizeAppConfigWireSnapshot (subpath) | appConfigTransport.ts:73 | fn | daemon |
| appSnapshotTraceAttrs (subpath) | appRuntimeTrace.ts:108 | fn | daemon |
| appInboxItemTraceAttrs (subpath) | appRuntimeTrace.ts:123 | fn | daemon |
| APP_CONFIG_TRACE_IDENTITY_KEYS (subpath) | appRuntimeTrace.ts:13 | const | daemon |
| APP_SOURCE_TRACE_IDENTITY_KEYS (subpath) | appRuntimeTrace.ts:20 | const | daemon |
| APP_SNAPSHOT_TRACE_IDENTITY_KEYS (subpath) | appRuntimeTrace.ts:31 | const | daemon |
| AppRuntimeTraceAttrs (subpath) | appRuntimeTrace.ts:4 | type | daemon |
| appConfigTraceAttrs (subpath) | appRuntimeTrace.ts:73 | fn | daemon |
| appSourceTraceAttrs (subpath) | appRuntimeTrace.ts:84 | fn | daemon |
| CLEANER_APP_ID (subpath) | apps/cleaner/configProtocol.ts:28 | const | daemon |
| CLEANER_NOTIFICATION_CLASS (subpath) | apps/cleaner/configProtocol.ts:30 | const | daemon |
| CLEANER_CONFIG_KEYS (subpath) | apps/cleaner/configProtocol.ts:54 | const | daemon |
| CLEANER_CONFIG_BOUNDS (subpath) | apps/cleaner/configProtocol.ts:61 | const | daemon |
| REMINDER_FIRE_REQUEST_CAPABILITY (subpath) | apps/reminder/protocol.ts:8 | const | daemon |
| AttentionHint | attentionDependencyOracle.ts:11 | type | daemon |
| Brand | brandedIds.ts:29 | type | cli |
| asMessageId | brandedIds.ts:56 | const | cli |
| AxSurfaceText | brandedIds.ts:71 | type | daemon |
| asAxSurfaceText | brandedIds.ts:79 | const | daemon |
| AgentLoginIntegrationInventoryProjection | capabilityInventories.ts:100 | type | cli |
| AGENT_LOGIN_INTEGRATION_INVENTORY_PROJECTION | capabilityInventories.ts:104 | const | cli |
| currentTimeMs | clock.ts:1 | fn | cli,computer,daemon |
| setClockTimeout | clock.ts:21 | fn | cli,computer,daemon |
| clearClockTimeout | clock.ts:25 | fn | computer,daemon |
| currentDate | clock.ts:5 | fn | cli,computer,daemon |
| createDaemonApiClient | daemonApiClient.ts:262 | fn | cli |
| DaemonApiClientFailure | daemonApiClient.ts:60 | interface | cli |
| DaemonApiClientResult | daemonApiClient.ts:67 | type | cli |
| DaemonApiRouteKey | daemonApiContract.ts:277 | type | cli |
| DaemonApiRequestQueryByRoute | daemonApiContract.ts:279 | type | cli |
| DaemonApiRequestBodyByRoute | daemonApiContract.ts:285 | type | cli |
| DaemonApiResponseByRoute | daemonApiContract.ts:291 | type | cli |
| DaemonApiRawTransport | daemonApiRawClient.ts:37 | interface | cli |
| buildDaemonApiRawRoutePath | daemonApiRawClient.ts:415 | fn | cli |
| DaemonApiContractRejectionDiagnostic | daemonApiRawClient.ts:67 | interface | cli |
| DaemonApiRawFailure | daemonApiRawClient.ts:81 | type | cli |
| ExternalRuntimeIntegrationManifest | externalAgentIntegration.ts:130 | type | cli |
| ExternalAgentWakeEventEnvelope | externalAgentIntegration.ts:176 | type | cli |
| ExternalAgentWakeAttemptInput | externalAgentIntegration.ts:178 | interface | cli |
| ExternalAgentWakeAdapter | externalAgentIntegration.ts:190 | interface | cli |
| validateExternalRuntimeIntegrationManifest | externalAgentIntegration.ts:195 | fn | cli |
| validateExternalAgentWakeEventEnvelope | externalAgentIntegration.ts:199 | fn | cli |
| EXTERNAL_AGENT_COMMS_PROTOCOL_VERSION | externalAgentIntegration.ts:3 | const | cli |
| EXTERNAL_AGENT_PROOF_SCHEMA_VERSION | externalAgentIntegration.ts:4 | const | cli |
| EXTERNAL_RUNTIME_INTEGRATION_MANIFEST_SCHEMA | externalAgentIntegration.ts:5 | const (string) | cli |
| EXTERNAL_AGENT_WAKE_EVENT_SCHEMA | externalAgentIntegration.ts:6 | const (string) | cli |
| ExternalAgentAdapterFailure | externalAgentIntegration.ts:67 | type | cli |
| AgentMessage | index.ts:100 | interface | daemon |
| BUILTIN_RUNTIME_PROVIDER_ENV_KEYS | index.ts:1003 | const | daemon |
| BUILTIN_RUNTIME_GATEWAY_PROVIDER_ENV_KEYS | index.ts:1025 | const | daemon |
| BUILTIN_RUNTIME_GATEWAY_PROVIDER_BASE_URL_ENV_KEYS | index.ts:1028 | const | daemon |
| RuntimeConfig | index.ts:1105 | type | daemon |
| AgentConfig | index.ts:1111 | interface | daemon |
| ProfileCreatedAgentSummary | index.ts:1198 | interface | cli |
| HumanProfileView | index.ts:1226 | interface | cli |
| AgentProfileView | index.ts:1241 | interface | cli |
| ProfileView | index.ts:1268 | type | cli |
| FileNode | index.ts:1270 | interface | daemon |
| WorkspaceDirectoryInfo | index.ts:1280 | interface | daemon |
| SkillInfo | index.ts:1292 | interface | daemon |
| AgentActivityKind | index.ts:1311 | type | daemon |
| AgentActivityDetailKind | index.ts:1368 | type | daemon |
| isAgentActivityDetailKind | index.ts:1369 | const | daemon |
| EXTERNAL_AGENT_ACTIVITY_EVENT_SCHEMA | index.ts:1389 | const (string) | cli |
| EXTERNAL_AGENT_ACTIVITY_DRAIN_SCHEMA | index.ts:1390 | const (string) | cli |
| EXTERNAL_AGENT_ACTIVITY_INGEST_SCHEMA | index.ts:1391 | const (string) | cli |
| EXTERNAL_AGENT_ACTIVITY_TEXT_LIMIT | index.ts:1393 | const | cli |
| EXTERNAL_AGENT_ACTIVITY_TOOL_NAME_LIMIT | index.ts:1394 | const | cli |
| ExternalAgentActivityEvent | index.ts:1407 | interface | cli |
| ExternalAgentActivityDrainResponse | index.ts:1435 | interface | cli |
| ExternalAgentActivityIngestRequest | index.ts:1441 | interface | cli |
| SubagentLineage | index.ts:1457 | interface | daemon |
| TrajectoryEntry | index.ts:1479 | type | daemon |
| DaemonTrajectoryEntry | index.ts:1493 | type | daemon |
| normalizeActivity | index.ts:1509 | fn | daemon |
| RUNTIMES | index.ts:1703 | const | daemon |
| isRuntimeDeprecated | index.ts:1756 | fn | cli |
| getRuntimeDisplayName | index.ts:1788 | fn | cli,daemon |
| RuntimeModelInfo | index.ts:1813 | interface | daemon |
| RuntimeModelSet | index.ts:1835 | interface | daemon |
| RuntimeModelSourceOutcome | index.ts:1857 | type | daemon |
| runtimeModelSourceOutcomeFromSet | index.ts:1870 | fn | daemon |
| getStaticRuntimeModelSourceSet | index.ts:2036 | fn | daemon |
| isBuiltInRuntimeProviderId | index.ts:2243 | fn | daemon |
| isBuiltInRuntimeGatewayProviderId | index.ts:2247 | fn | daemon |
| ReminderStatus | index.ts:253 | type | cli |
| hydrateRuntimeConfig | index.ts:2577 | fn | daemon |
| ReminderJob | index.ts:273 | interface | daemon |
| runtimeConfigModelValue | index.ts:2739 | fn | daemon |
| runtimeConfigToLaunchFields | index.ts:2825 | fn | daemon |
| ReminderSummary | index.ts:286 | interface | cli |
| TASK_CLAIM_REASON_ALREADY_CLAIMED_BY_YOU | index.ts:2901 | const | cli |
| ReminderEventSummary | index.ts:313 | interface | cli |
| AgentRuntimeProfileRef | index.ts:329 | interface | daemon |
| AgentRuntimeProfileReport | index.ts:338 | interface | daemon |
| RuntimeProfileReportSource | index.ts:348 | type | daemon |
| AgentMigrationTransportReady | index.ts:362 | interface | daemon |
| COMPUTER_CAPABILITY_SUPERVISOR_MUTATIONS | index.ts:418 | const | daemon |
| ComputerLifecycleAction | index.ts:423 | type | computer |
| ComputerLifecycleExecutionAck | index.ts:432 | interface | computer,daemon |
| WIKI_AGENT_WORKSPACE_ENV | index.ts:447 | const | daemon |
| WIKI_AGENT_WORKSPACE_ENABLED | index.ts:448 | const | daemon |
| WIKI_WORKSPACE_PACK_PROTOCOL_VERSION | index.ts:449 | const | daemon |
| WIKI_WORKSPACE_PACK_CAPABILITY | index.ts:450 | const | daemon |
| WikiWorkspacePack | index.ts:464 | interface | daemon |
| WikiWorkspaceFileReceipt | index.ts:470 | interface | daemon |
| WikiWorkspaceEnsureReceipt | index.ts:476 | interface | daemon |
| canonicalizeWikiWorkspacePackFiles | index.ts:482 | fn | daemon |
| MentionDeliveryIdentitySnapshot | index.ts:524 | type | daemon |
| MentionDeliveryTransitionStage | index.ts:532 | type | daemon |
| MentionDeliveryTerminalErrorCode | index.ts:537 | type | daemon |
| ServerToMachineMessage | index.ts:545 | type (WS union, 155 lines) | daemon |
| AgentMigrationTransportLeaseMessage | index.ts:701 | type | daemon |
| RuntimeErrorClass | index.ts:723 | type | daemon |
| RuntimeErrorReason | index.ts:739 | type | daemon |
| RuntimeErrorReasonProvenance | index.ts:746 | type | daemon |
| RuntimeErrorActivityDiagnostic | index.ts:748 | interface | daemon |
| FeedbackTranscriptReportTimeSource | index.ts:758 | type | daemon |
| FeedbackTranscriptWindow | index.ts:765 | interface | daemon |
| MachineToServerMessage | index.ts:775 | type (WS union, 129 lines) | daemon |
| MachineShutdownReason | index.ts:905 | type | daemon |
| validateKnowledgeContext | knowledgeContext.ts:20 | fn | cli |
| RAFT_CLIENT_CAPABILITIES_HEADER | knowledgeContext.ts:3 | const | cli |
| MANUAL_CONTEXT_CAPABILITY | knowledgeContext.ts:4 | const | cli |
| MANUAL_INDEX_COMMAND | knowledgeContext.ts:5 | const | cli |
| ManagedMcpCallRequest | managedMcp.ts:104 | interface | daemon |
| ManagedMcpCallResult | managedMcp.ts:116 | interface | daemon |
| ManagedMcpRuntimeTool | managedMcp.ts:86 | interface | daemon |
| ManagedMcpRuntimeSnapshot | managedMcp.ts:99 | interface | daemon |
| OAUTH_CLIENT_CATEGORIES | oauthClientCategories.ts:1 | const | cli |
| OAuthClientCategory | oauthClientCategories.ts:20 | type | cli |
| canonicalizeOAuthClientCategory | oauthClientCategories.ts:33 | fn | cli |
| formatProducerFactLineageBracket | producerFactLineage.ts:35 | fn | daemon |
| isProviderConnectionProviderId | providerConnections.ts:53 | fn | daemon |
| structuredRaftMentionStillAppears | raftRefs.ts:366 | fn | cli |
| RaftTargetString | raftRefs.ts:81 | type | cli |
| RuntimeAccountUsageHealth | runtimeAccountUsage.ts:15 | type | daemon |
| RuntimeAccountUsageSnapshot | runtimeAccountUsage.ts:159 | type | daemon |
| RUNTIME_ACCOUNT_USAGE_PROTOCOL_VERSION | runtimeAccountUsage.ts:3 | const | daemon |
| maskRuntimeAccountEmail | runtimeAccountUsage.ts:34 | fn | daemon |
| RuntimeAccountUsageProvider | runtimeAccountUsage.ts:6 | type | daemon |
| formatRuntimeProviderModelLabel | runtimeProviderDisplay.ts:11 | fn | daemon |
| renderThirdPartyInertJson | thirdPartyInertRenderer.ts:115 | fn | cli,daemon |
| resolveToolSemantic | toolDisplay.ts:181 | fn | daemon |
| normalizeToolDisplayInvocation | toolDisplay.ts:468 | fn | daemon |
| getToolActivityLabel | toolDisplay.ts:478 | fn | daemon |
| summarizeToolInput | toolDisplay.ts:490 | fn | daemon |
| formatTraceparent | tracing/index.ts:124 | fn | daemon |
| parseTraceparent | tracing/index.ts:129 | fn | daemon |
| TraceEvent | tracing/index.ts:16 | interface | trace-client |
| noopTracer | tracing/index.ts:189 | const | computer,daemon |
| CompletedTraceSpan | tracing/index.ts:22 | interface | trace-client |
| TraceScope | tracing/index.ts:262 | interface | daemon |
| TraceSpanAttrContracts | tracing/index.ts:307 | type | daemon |
| createTraceScopeTracer | tracing/index.ts:314 | fn | daemon |
| BasicTracer | tracing/index.ts:460 | class | trace-client |
| StartSpanOptions | tracing/index.ts:54 | interface | trace-client |
| TraceStatus | tracing/index.ts:6 | type | daemon,trace-client |
| EndSpanOptions | tracing/index.ts:62 | interface | trace-client |
| ActiveSpan | tracing/index.ts:66 | interface | daemon,trace-client |
| TraceAttributes | tracing/index.ts:7 | type | trace-client |
| Tracer | tracing/index.ts:72 | interface | computer,daemon,trace-client |
| TraceSink | tracing/index.ts:76 | interface | trace-client |
| formatUtcTimestamp | utcTimestamp.ts:1 | fn | cli,daemon |

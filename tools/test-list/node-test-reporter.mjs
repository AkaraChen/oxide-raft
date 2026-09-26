// node:test reporter for tools/test-list: one NDJSON line per executed test
// case (suites excluded) with its file, describe path, title, and outcome.
// A failing suite (a describe whose hook or body threw, or a file that failed
// to load) becomes an `{ file, error }` line so tools/test-list fails loudly.
export default async function* testListReporter(source) {
  const stacks = new Map();
  for await (const event of source) {
    const data = event.data ?? {};
    if (!data.file) continue;
    const stack = stacks.get(data.file) ?? [];
    stacks.set(data.file, stack);
    if (event.type === "test:start") {
      stack.length = data.nesting;
      stack[data.nesting] = data.name;
      continue;
    }
    if (event.type !== "test:pass" && event.type !== "test:fail") continue;
    if (data.details?.type === "suite") {
      if (event.type === "test:fail") yield `${JSON.stringify({ file: data.file, error: `suite failed: ${[...stack.slice(0, data.nesting), data.name].join(" > ")}: ${String(data.details?.error?.message ?? data.details?.error ?? "").split("\n")[0]}` })}\n`;
      continue;
    }
    const outcome = data.skip !== undefined ? "skipped" : data.todo !== undefined ? "todo" : event.type === "test:pass" ? "passed" : "failed";
    yield `${JSON.stringify({
      file: data.file,
      describe: stack.slice(0, data.nesting),
      title: data.name,
      outcome,
      ...(typeof data.skip === "string" ? { skipReason: data.skip } : {}),
    })}\n`;
  }
}

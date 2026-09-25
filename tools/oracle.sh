#!/usr/bin/env bash
# Runs the pinned upstream TypeScript sources as the behavioral oracle.
#
#   tools/oracle.sh setup                 install upstream's locked deps (cli, daemon, computer closures)
#   tools/oracle.sh raft <args...>        run the upstream `raft` CLI
#   tools/oracle.sh computer <args...>    run the upstream `raft-computer` CLI
#   tools/oracle.sh test <pkg> <file...>  run upstream tests (pkg: cli | daemon | computer | shared)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
upstream="$root/upstream/raft-source"
cmd="${1:-}"
shift || true

case "$cmd" in
  setup)
    cd "$upstream"
    pnpm install --frozen-lockfile --ignore-scripts \
      --filter "@botiverse/raft..." \
      --filter "@botiverse/raft-daemon..." \
      --filter "@botiverse/raft-computer..."
    ;;
  raft)
    cd "$upstream/packages/cli"
    exec env -u SLOCK_CLI_TRANSPORT_DIR node --import tsx src/index.ts "$@"
    ;;
  computer)
    cd "$upstream/packages/computer"
    exec node --import tsx src/index.ts "$@"
    ;;
  test)
    pkg="$1"; shift
    cd "$upstream/packages/$pkg"
    case "$pkg" in
      cli|shared) exec node --import tsx --test "$@" ;;
      daemon|computer) exec npx vitest run "$@" ;;
      *) echo "unknown package: $pkg" >&2; exit 2 ;;
    esac
    ;;
  *)
    sed -n '2,8p' "$0" >&2
    exit 2
    ;;
esac

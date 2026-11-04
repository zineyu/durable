#!/usr/bin/env bash
# 生成 TS 协议类型与协议类型清单。产物必须提交，durable-test 会比对。
set -euo pipefail

cd "$(dirname "$0")/.."

# 文件名的排序决定清单里各节的顺序：强制 C 排序，任何机器与地区设置产出同一份清单。
export LC_ALL=C

generated=apps/client/src/api/generated
manifest=docs/api/types.md

rm -f "$generated"/*.ts
bash scripts/task.sh typegen

mkdir -p "$(dirname "$manifest")"
{
  cat <<'HEADER'
# 协议类型清单

由 `devenv shell -- durable-typegen` 从 Rust 类型生成，与 `apps/client/src/api/generated/` 同一来源。不要手改。

HEADER
  for file in "$generated"/*.ts; do
    name=$(basename "$file" .ts)
    printf '## %s\n\n```ts\n' "$name"
    awk 'NR > 1 && !/^import type / { print }' "$file" | sed -e '/./,$!d'
    printf '```\n\n'
  done
} > "$manifest"

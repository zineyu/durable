#!/usr/bin/env bash
# 布局门禁：apps/ 与 packages/ 下只放能被发现的包，crates/ 下只放库 crate。
# 包是否被发现的判据是 scripts/task.sh --list 的输出，本脚本不重复实现发现逻辑。
# 已跟踪与未跟踪的文件一并算入：写完文件立刻跑本脚本，与提交前跑，结果必须相同。
set -euo pipefail

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

bash scripts/task.sh --list | cut -f3 | sort > "$work/discovered.txt"

failed=0

for root in apps packages; do
  [ -d "$root" ] || continue
  for entry in "$root"/*/; do
    [ -d "$entry" ] || continue
    dir=${entry%/}
    if ! grep -qxF "$dir" "$work/discovered.txt"; then
      printf '%s：不是被发现的包。放 Cargo.toml 或 package.json，或删除该目录。\n' "$dir" >&2
      failed=1
    fi
  done
done

for entry in crates/*/; do
  [ -d "$entry" ] || continue
  dir=${entry%/}
  if [ -f "$dir/src/main.rs" ]; then
    printf '%s：crates/ 只放库 crate。可执行的端放 apps/。\n' "$dir" >&2
    failed=1
  fi
  if grep -qE '^[[:space:]]*\[\[bin\]\]' "$dir/Cargo.toml"; then
    printf '%s：crates/ 只放库 crate。Cargo.toml 里不要声明 [[bin]]。\n' "$dir" >&2
    failed=1
  fi
done

bash scripts/task.sh --list | cut -f2 | sort | uniq -d > "$work/duplicates.txt"
if [ -s "$work/duplicates.txt" ]; then
  printf '这些包名被多次发现，任务无法按名称定位：\n' >&2
  sed 's/^/  /' "$work/duplicates.txt" >&2
  failed=1
fi

if [ "$failed" -ne 0 ]; then
  exit 1
fi

printf '布局：apps/ 与 packages/ 下的目录都是被发现的包，crates/ 下只有库 crate。\n'

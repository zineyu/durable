#!/usr/bin/env bash
# 依赖白名单：每个包的直接依赖都必须登记在 deps.allow，未登记即失败。
set -euo pipefail

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo metadata --no-deps --format-version 1 --locked |
  jq -r '.packages[] | .name as $package | .dependencies[] | "\($package) \(.name)"' |
  sort -u > "$work/actual"

awk '
  {
    entry = $0
    sub(/#.*/, "", entry)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", entry)
    if (entry == "") next
    if (split(entry, field, /[[:space:]]+/) != 2) {
      printf "deps.allow 第 %d 行格式应为「<package> <crate> # 理由」：%s\n", NR, $0 > "/dev/stderr"
      failed = 1
      next
    }
    print field[1] " " field[2]
  }
  END {
    exit failed
  }
' deps.allow | sort -u > "$work/allowed"

status=0

undeclared=$(comm -13 "$work/allowed" "$work/actual")
if [ -n "$undeclared" ]; then
  printf '未登记的依赖，请先在 deps.allow 补一行「<package> <crate> # 理由」：\n' >&2
  printf '%s\n' "$undeclared" | sed 's/^/  /' >&2
  status=1
fi

stale=$(comm -23 "$work/allowed" "$work/actual")
if [ -n "$stale" ]; then
  printf 'deps.allow 里已不存在的依赖，请删除对应行：\n' >&2
  printf '%s\n' "$stale" | sed 's/^/  /' >&2
  status=1
fi

if [ "$status" -eq 0 ]; then
  echo "依赖白名单：与 Cargo 清单一致。"
fi

exit "$status"

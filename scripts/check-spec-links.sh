#!/usr/bin/env bash
# 规格追溯：代码里引用的 user story 编号必须在 docs/specs 里存在。
# 未被任何代码引用的编号只打印清单，不阻断。
set -euo pipefail

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for spec in docs/specs/*.md; do
  name=$(basename "$spec")
  grep -oE '^[0-9]+\. ' "$spec" | tr -d '. ' | sort -u > "$work/${name}.numbers"
done
cat "$work"/*.numbers | sort -u > "$work/defined.txt"

git grep -nE '规格 user story' -- crates client |
  while IFS=: read -r file line rest; do
    for number in $(printf '%s\n' "$rest" | grep -oE 'user story [0-9]+( 与 [0-9]+)?' | grep -oE '[0-9]+'); do
      printf '%s:%s %s\n' "$file" "$line" "$number"
    done
  done > "$work/refs.txt" || true

status=0
while read -r where number; do
  if ! grep -qx "$number" "$work/defined.txt"; then
    printf '%s: 引用 user story %s，但 docs/specs 里没有该编号\n' "$where" "$number"
    status=1
  fi
done < "$work/refs.txt"

awk '{ print $2 }' "$work/refs.txt" | sort -u > "$work/referenced.txt"

for spec in docs/specs/*.md; do
  name=$(basename "$spec")
  unreferenced=$(comm -23 "$work/${name}.numbers" "$work/referenced.txt" | sort -n | tr '\n' ' ')
  if [ -n "$unreferenced" ]; then
    printf '%s 未被引用的编号：%s\n' "$name" "$unreferenced"
  fi
done

exit "$status"

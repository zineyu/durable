#!/usr/bin/env bash
# 规模预算：单个文件超过默认上限即失败，源码与集成测试同一套上限。
# 超限文件必须在 scripts/source-budget.allow 登记，登记值不得再增大。
set -euo pipefail

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git ls-files 'crates/*/src/*.rs' 'crates/*/tests/*.rs' 'client/src/*.ts' 'client/src/*.tsx' |
  grep -Ev '^client/src/api/generated/|\.test\.tsx?$' |
  sort -u > "$work/files"

xargs wc -l < "$work/files" > "$work/sizes"

awk -v ALLOW=scripts/source-budget.allow '
  BEGIN {
    rust_limit = 600
    ts_limit = 400
    bad = 0
    entry = 0
    while ((getline line < ALLOW) > 0) {
      entry += 1
      sub(/#.*/, "", line)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
      if (line == "") continue
      if (split(line, field, /[[:space:]]+/) != 2) {
        printf "scripts/source-budget.allow 第 %d 行格式应为「<path> <max>」：%s\n", entry, line
        bad = 1
        continue
      }
      limit[field[1]] = field[2]
    }
    close(ALLOW)
  }
  $2 == "total" { next }
  {
    file = $2
    lines = $1 + 0
    fallback = (file ~ /\.tsx?$/) ? ts_limit : rust_limit
    if (file in limit) {
      max = limit[file] + 0
      if (lines > max) {
        printf "%s: %d 行，超过登记上限 %d。拆分文件。\n", file, lines, max
        bad = 1
      } else if (lines <= fallback) {
        printf "%s: %d 行，已回到默认上限 %d 以内，请从 scripts/source-budget.allow 删除该行。\n", file, lines, fallback
        bad = 1
      }
      delete limit[file]
    } else if (lines > fallback) {
      printf "%s: %d 行，超过默认上限 %d。拆分文件，或在 scripts/source-budget.allow 登记（只许变短）。\n", file, lines, fallback
      bad = 1
    }
  }
  END {
    for (path in limit) {
      printf "scripts/source-budget.allow 登记的 %s 不存在，请删除该行。\n", path
      bad = 1
    }
    exit bad
  }
' "$work/sizes"

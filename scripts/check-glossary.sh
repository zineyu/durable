#!/usr/bin/env bash
# 术语表自检：docs/glossary.md 里标识符形状的反引号术语必须能在源码里找到。
# 已跟踪与未跟踪的文件一并算入：写完文件立刻跑本脚本，与提交前跑，结果必须相同。
set -euo pipefail

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mark=$(printf '\140')
grep -oE "${mark}[^${mark}]+${mark}" docs/glossary.md | tr -d "$mark" | sort -u > "$work/terms.txt"
: > "$work/missing.txt"

while read -r term; do
  case "$term" in
    '' | *[!A-Za-z0-9_:.]*) continue ;;
  esac
  if ! git grep -qF --untracked -- "$term" -- crates apps; then
    printf '%s\n' "$term" >> "$work/missing.txt"
  fi
done < "$work/terms.txt"

if [ -s "$work/missing.txt" ]; then
  printf '术语表里这些术语在 crates/ 与 apps/ 中找不到，可能已过时：\n' >&2
  sed 's/^/  /' "$work/missing.txt" >&2
  exit 1
fi

printf '术语表：标识符术语都在源码中存在。\n'

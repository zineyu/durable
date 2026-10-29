#!/usr/bin/env bash
# 规格追溯：代码里引用的 user story 编号必须在 docs/specs 里存在，
# 且前缀与所在目录匹配：crates/executor 引用 R-，
# 其余 crate 允许 A- 与 R-。
# 未被任何代码引用的编号只打印清单，不阻断。
set -euo pipefail

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

spec_of() {
  case "$1" in
    A) printf 'docs/specs/rust-react-coding-agent.md' ;;
    R) printf 'docs/specs/remote-ssh-process-model.md' ;;
  esac
}

for prefix in A R; do
  spec=$(spec_of "$prefix")
  if [ ! -f "$spec" ]; then
    printf '规格追溯：找不到 %s（前缀 %s）。\n' "$spec" "$prefix" >&2
    exit 1
  fi
  grep -oE "^- \*\*${prefix}-[0-9]+\*\*" "$spec" | tr -d '*' | sed 's/^- //' | sort -u > "$work/defined-${prefix}.txt"
  if [ ! -s "$work/defined-${prefix}.txt" ]; then
    printf '规格追溯：%s 里没有 %s-N 形式的编号。\n' "$spec" "$prefix" >&2
    exit 1
  fi
done

for spec in docs/specs/*.md; do
  case "$spec" in
    docs/specs/rust-react-coding-agent.md | docs/specs/remote-ssh-process-model.md) ;;
    *)
      printf '规格追溯：%s 未分配前缀。在 scripts/check-spec-links.sh 的 spec_of 里为它指定 A- 或 R-。\n' "$spec" >&2
      exit 1
      ;;
  esac
done

: > "$work/refs.txt"
: > "$work/problems.txt"

git grep -nE '规格 user story' -- crates client |
  while IFS=: read -r file line rest; do
    case "$file" in
      crates/executor/*) prefix=R ;;
      *) prefix=AR ;;
    esac
    found=$(printf '%s\n' "$rest" | sed 's/.*规格 user story//' | grep -oE '[AR]-[0-9]+' || true)
    if [ -z "$found" ]; then
      printf '%s:%s 写了「规格 user story」，但后面没有 A-N 或 R-N 编号\n' "$file" "$line" >> "$work/problems.txt"
      continue
    fi
    for id in $found; do
      printf '%s:%s %s %s\n' "$file" "$line" "$prefix" "$id" >> "$work/refs.txt"
    done
  done

while read -r where allowed id; do
  prefix=${id%%-*}
  case "$allowed" in
    *"$prefix"*) ;;
    *)
      printf '%s: 引用 %s，但这里只允许 %s 前缀的编号\n' "$where" "$id" "$allowed" >> "$work/problems.txt"
      continue
      ;;
  esac
  if ! grep -qx "$id" "$work/defined-${prefix}.txt"; then
    printf '%s: 引用 user story %s，但 %s 里没有该编号\n' "$where" "$id" "$(spec_of "$prefix")" >> "$work/problems.txt"
  fi
done < "$work/refs.txt"

if [ -s "$work/problems.txt" ]; then
  cat "$work/problems.txt" >&2
  exit 1
fi

awk '{ print $3 }' "$work/refs.txt" | sort -u > "$work/referenced.txt"

for prefix in A R; do
  unreferenced=$(comm -23 "$work/defined-${prefix}.txt" "$work/referenced.txt" | sort -t- -k2,2n | tr '\n' ' ')
  if [ -n "$unreferenced" ]; then
    printf '%s 未被引用的编号：%s\n' "$(spec_of "$prefix")" "$unreferenced"
  fi
done

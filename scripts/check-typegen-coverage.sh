#!/usr/bin/env bash
# 类型导出覆盖：每个 #[derive(TS)] 的类型都必须有生成的 TS 定义。
# 缺文件说明该类型没登记到 export_types，客户端拿不到它的形状。
set -euo pipefail

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

generated=client/src/api/generated

cat > "$work/derived.awk" <<'AWK'
FNR == 1 { pending = 0 }
/^#\[derive\(/ {
  if ($0 ~ /(^|[^A-Za-z0-9_])TS([^A-Za-z0-9_]|$)/) pending = 1
  next
}
/^#\[/ { next }
pending && $0 ~ /^(pub(\([^)]*\))? )?(struct|enum) / {
  name = $0
  sub(/.*(struct|enum) /, "", name)
  sub(/[^A-Za-z0-9_].*/, "", name)
  print name
  pending = 0
  next
}
{ pending = 0 }
AWK

git ls-files -z 'crates/*/src/*.rs' |
  xargs -0 awk -f "$work/derived.awk" |
  sort -u > "$work/derived.txt"

if [ ! -s "$work/derived.txt" ]; then
  printf '类型导出覆盖：没找到任何 #[derive(TS)] 类型，检查脚本的匹配规则是否失效。\n' >&2
  exit 1
fi

: > "$work/missing.txt"
count=0
while read -r name; do
  count=$((count + 1))
  if [ ! -f "$generated/$name.ts" ]; then
    printf '%s\n' "$name" >> "$work/missing.txt"
  fi
done < "$work/derived.txt"

if [ -s "$work/missing.txt" ]; then
  printf '这些类型派生了 TS，但 %s/ 里没有对应文件：\n' "$generated" >&2
  sed 's/^/  /' "$work/missing.txt" >&2
  printf '在 crates/agent/src/lib.rs 的 export_types 里补 <类型>::export_all(&config)。\n' >&2
  exit 1
fi

printf '类型导出覆盖：%d 个 #[derive(TS)] 类型都有生成的 TS 定义。\n' "$count"

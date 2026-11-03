#!/usr/bin/env bash
# 单一写入线的守门。崩溃前缀矩阵的完备性建立在「全部持久化写都经过 Store::commit」上：
# 绕过它一次，提交点副本就不再等于崩溃点集合，矩阵的全覆盖声明也就失效。见 ADR 0056。
set -euo pipefail

cd "$(dirname "$0")/.."

status=0

# 全仓计数一个正则的命中数。`--count-matches` 按文件输出 `file:count`。
count_matches() {
  local hits
  hits=$(rg --count-matches --no-heading "$@" 2>/dev/null || true)
  printf '%s\n' "$hits" | awk -F: '{sum += $2} END {print sum + 0}'
}

check_one() {
  local pattern=$1
  local why=$2
  shift 2
  local hits
  hits=$(count_matches -g 'crates/*/src/**' "$@" -- "$pattern")
  if [ "$hits" -ne 1 ]; then
    printf '单一写入线被破坏：%s\n  期望 1 处，实际 %s 处：%s\n' "$why" "$hits" "$pattern" >&2
    rg -n -g 'crates/*/src/**' "$@" -- "$pattern" >&2 || true
    status=1
  fi
}

check_none() {
  local pattern=$1
  local why=$2
  shift 2
  local hits
  hits=$(count_matches -g 'crates/*/src/**' "$@" -- "$pattern")
  if [ "$hits" -ne 0 ]; then
    printf '单一写入线被破坏：%s\n  期望 0 处，实际 %s 处：%s\n' "$why" "$hits" "$pattern" >&2
    rg -n -g 'crates/*/src/**' "$@" -- "$pattern" >&2 || true
    status=1
  fi
}

# 写事务只由 Store::commit 发起；变更只由 apply 逐条落库。
check_one '\.apply\(changes' '写事务只能由 Store::commit 发起'
check_one 'apply_change\(&mut transaction' '变更只能由 apply 逐条落库'

# storage/ 之外不出现 SQL 与 JSON 抽取：读也归存储层，写没有第二条通道。
check_none 'sqlx::query' 'SQL 只出现在 storage/ 里' -g '!crates/*/src/storage/**'
check_none 'INSERT INTO|UPDATE [a-z_]+ SET|DELETE FROM' '写 SQL 只出现在 storage/ 里' -g '!crates/*/src/storage/**'

if [ "$status" -ne 0 ]; then
  exit 1
fi

printf '单一写入线：写事务、变更落库与 SQL 都在预期位置。\n'

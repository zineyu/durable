#!/usr/bin/env bash
# 外部通道门禁：网络、子进程、文件写入、后台任务只允许出现在白名单里。
# 白名单是 scripts/external-channels.allow，逐文件逐类登记并写理由。
# 范围是产品代码：crates/*/src 与 apps/*/src。测试、Storybook 与生成物不在范围内，
# 理由见 docs/adr/0057-external-channel-gate.md。
# 已跟踪与未跟踪的文件一并算入：写完文件立刻跑本脚本，与提交前跑，结果必须相同。
set -euo pipefail

cd "$(dirname "$0")/.."

allow_file=scripts/external-channels.allow
rules=(network subprocess file-write background)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# 每一类通道的识别式。命中即登记，因此识别式宁可窄：只认会真正建立通道的调用。
rule_patterns() {
  case "$1" in
    network)
      cat <<'PATTERNS'
(^|[^A-Za-z0-9_])reqwest(::|_)
(^|[^A-Za-z0-9_])(TcpListener|TcpStream|UdpSocket)\b
axum::serve
(^|[^A-Za-z0-9_])fetch\(
EventSource
(^|[^A-Za-z0-9_])WebSocket\b
(^|[^A-Za-z0-9_])XMLHttpRequest\b
sendBeacon
PATTERNS
      ;;
    subprocess)
      cat <<'PATTERNS'
(^|[^A-Za-z0-9_])Command::new\(
(^|[^A-Za-z0-9_])process::(Command|Stdio|Child)\b
(^|[^A-Za-z0-9_])tokio::process\b
(^|[^A-Za-z0-9_])libc::(fork|exec|kill|waitpid)\b
child_process
PATTERNS
      ;;
    file-write)
      cat <<'PATTERNS'
(^|[^A-Za-z0-9_])fs::(write|append|create_dir|remove_file|remove_dir|rename|copy|hard_link|set_permissions|set_len)
(^|[^A-Za-z0-9_])File::(create|options)\(
(^|[^A-Za-z0-9_])OpenOptions\b
RollingFileAppender
create_if_missing\(
localStorage
sessionStorage
indexedDB
PATTERNS
      ;;
    background)
      cat <<'PATTERNS'
(^|[^A-Za-z0-9_])tokio::spawn
(^|[^A-Za-z0-9_])spawn_blocking
(^|[^A-Za-z0-9_])thread::spawn
(^|[^A-Za-z0-9_])JoinSet\b
PATTERNS
      ;;
  esac
}

trim() {
  local value=$1
  value=${value#"${value%%[![:space:]]*}"}
  printf '%s' "${value%"${value##*[![:space:]]}"}"
}

# 去掉注释：整行注释直接丢弃；行内注释切到 `//` 之前，`//` 前面是 `:` 时不切，
# 避免把 `https://` 当成注释。
strip_comment() {
  local line
  line=$(trim "$1")
  if [[ $line == '//'* || $line == '/*'* || $line == '*'* ]]; then
    printf '\n'
    return
  fi
  printf '%s\n' "$1" | sed -E 's@([^:])(//|/\*).*@\1@'
}

# 在一个文件里查「路径 + 类别」：$3 是类别的列号，命中的记录与白名单的列不同。
contains_pair() {
  awk -v path="$1" -v rule="$2" -v column="$4" '
    $1 == path && $column == rule { found = 1 }
    END { exit !found }
  ' "$3"
}

{
  git ls-files 'crates/*/src/*.rs' 'apps/*/src/*.rs' 'apps/client/src/*.ts' 'apps/client/src/*.tsx'
  git ls-files --others --exclude-standard 'crates/*/src/*.rs' 'apps/*/src/*.rs' 'apps/client/src/*.ts' 'apps/client/src/*.tsx'
} | grep -Ev '\.test\.tsx?$|\.stories\.tsx?$|\.fixture\.tsx?$|^apps/client/src/api/generated/' |
  sort -u > "$work/files" || true

if [ ! -s "$work/files" ]; then
  printf '外部通道门禁：没找到任何被检查的文件，检查脚本的范围规则是否失效。\n' >&2
  exit 1
fi

: > "$work/hits"
for rule in "${rules[@]}"; do
  args=()
  while IFS= read -r pattern; do
    args+=(-e "$pattern")
  done < <(rule_patterns "$rule")
  xargs -r rg -n --no-heading --color never -H "${args[@]}" < "$work/files" |
    while IFS= read -r hit; do
      file=${hit%%:*}
      rest=${hit#*:}
      line=${rest%%:*}
      text=${rest#*:}
      if printf '%s\n' "$(strip_comment "$text")" | rg --color never "${args[@]}" > /dev/null; then
        printf '%s %s %s %s\n' "$file" "$line" "$rule" "$text" >> "$work/hits"
      fi
    done || true
done

if [ ! -s "$work/hits" ]; then
  printf '外部通道门禁：没有命中任何通道，检查脚本的识别式是否失效。\n' >&2
  exit 1
fi

: > "$work/allowed"
: > "$work/problems"
: > "$work/stale"
: > "$work/violations"

if [ ! -f "$allow_file" ]; then
  printf '外部通道门禁：找不到白名单 %s。\n' "$allow_file" >&2
  exit 1
fi

number=0
while IFS= read -r raw || [ -n "$raw" ]; do
  number=$((number + 1))
  line=$(trim "$raw")
  if [ -z "$line" ]; then
    continue
  fi
  if [[ $line == '#'* ]]; then
    continue
  fi
  if [[ $raw != *'#'* ]]; then
    printf 'external-channels.allow 第 %d 行缺少理由：应为「<路径> <类别> # <理由>」。\n' "$number" >> "$work/problems"
    continue
  fi
  entry=$(trim "${raw%%#*}")
  reason=$(trim "${raw#*#}")
  read -r path rule extra <<< "$entry"
  if [ -z "$reason" ]; then
    printf 'external-channels.allow 第 %d 行的理由为空。\n' "$number" >> "$work/problems"
    continue
  fi
  if [ -z "${path:-}" ] || [ -z "${rule:-}" ] || [ -n "${extra:-}" ]; then
    printf 'external-channels.allow 第 %d 行应为「<路径> <类别> # <理由>」：%s\n' "$number" "$entry" >> "$work/problems"
    continue
  fi
  if [[ " ${rules[*]} " != *" $rule "* ]]; then
    printf 'external-channels.allow 第 %d 行的类别不在 %s 里：%s\n' "$number" "${rules[*]}" "$rule" >> "$work/problems"
    continue
  fi
  if grep -qxF "$path $rule" "$work/allowed"; then
    printf 'external-channels.allow 第 %d 行重复登记：%s %s\n' "$number" "$path" "$rule" >> "$work/problems"
    continue
  fi
  printf '%s %s\n' "$path" "$rule" >> "$work/allowed"
  if ! grep -qxF "$path" "$work/files"; then
    printf '外部通道白名单里的文件不在检查范围（已删除、已改名，或属于测试与生成物）：%s\n' "$path" >> "$work/stale"
  elif ! contains_pair "$path" "$rule" "$work/hits" 3; then
    printf '外部通道白名单里的条目已无命中，请删除：%s %s\n' "$path" "$rule" >> "$work/stale"
  fi
done < "$allow_file"

while IFS= read -r hit; do
  read -r file line rule text <<< "$hit"
  if ! contains_pair "$file" "$rule" "$work/allowed" 2; then
    printf '%s:%s: 规则 %s：%s\n' "$file" "$line" "$rule" "$text" >> "$work/violations"
  fi
done < "$work/hits"

status=0
if [ -s "$work/problems" ]; then
  printf '白名单格式错误：\n' >&2
  sed 's/^/  /' "$work/problems" >&2
  status=1
fi
if [ -s "$work/violations" ]; then
  printf '外部通道的命中未登记。先在 scripts/external-channels.allow 补一行「<路径> <类别> # 理由」，再提交：\n' >&2
  sort "$work/violations" | sed 's/^/  /' >&2
  status=1
fi
if [ -s "$work/stale" ]; then
  printf '白名单与代码不一致：\n' >&2
  sed 's/^/  /' "$work/stale" >&2
  status=1
fi

if [ "$status" -ne 0 ]; then
  exit 1
fi

printf '外部通道门禁：%d 个文件、%d 条登记与命中一致。\n' \
  "$(cut -d' ' -f1 "$work/allowed" | sort -u | wc -l | tr -d ' ')" \
  "$(wc -l < "$work/allowed" | tr -d ' ')"

#!/usr/bin/env bash
# 跨包任务编排：发现仓库里的包，按每个包声明的入口执行同一个任务。
# 只回答「有哪些包」与「某任务在某个包里怎么跑」；仓库级检查（fmt、deny、typos、文档）与生成物比对
# 不在本脚本，见 scripts/lint-fast.sh、scripts/lint.sh 与 scripts/typegen.sh。
#
# 包发现：Rust 包来自 cargo metadata，Node 包来自 apps/*/package.json 与 packages/*/package.json。
# 执行顺序固定为 Node 包在前、Rust 包在后：agent 在编译期内嵌 apps/client/dist。
set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
  cat <<'USAGE'
用法：scripts/task.sh <任务> [--package <包名>] [--kind rust|node] [--list]

任务：
  build     编译包
  lint      包的静态检查
  test      包的测试。先构建所有 Node 包，Rust 包在编译期内嵌它们的产物
  e2e       包的端到端测试。前置与 test 相同
  typegen   包的协议类型导出

没有声明某个任务的包被跳过，不算失败。--list 按「类别、包名、目录、任务」四列打印发现结果。
USAGE
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

packages="$work/packages.tsv"

cargo metadata --no-deps --format-version 1 --locked |
  jq -r --arg root "$PWD" '.packages[] | [
    "rust",
    .name,
    (.manifest_path | sub("/Cargo.toml$"; "") | sub("^" + $root + "/"; "")),
    ([.targets[] | select(any(.kind[]; . == "bin")) | .name] | join(","))
  ] | @tsv' > "$work/rust.tsv"

: > "$work/node.tsv"
for manifest in apps/*/package.json packages/*/package.json; do
  [ -f "$manifest" ] || continue
  jq -r --arg dir "$(dirname "$manifest")" '[
    "node", .name, $dir, ((.scripts // {}) | keys | join(","))
  ] | @tsv' "$manifest" >> "$work/node.tsv"
done

cat "$work/node.tsv" "$work/rust.tsv" > "$packages"

has_item() {
  case ",$1," in
    *",$2,"*) return 0 ;;
    *) return 1 ;;
  esac
}

tasks_of() {
  local kind=$1
  local name=$2
  local entry=$3
  local item
  local found=""
  case "$kind" in
    node)
      has_item "$entry" build && found="$found build"
      has_item "$entry" test && found="$found test"
      has_item "$entry" test:e2e && found="$found e2e"
      for item in lint lint:css typecheck; do
        if has_item "$entry" "$item"; then
          found="$found lint"
          break
        fi
      done
      ;;
    rust)
      found="build lint test"
      if has_item "$entry" "export-types-$name"; then
        found="$found typegen"
      fi
      ;;
  esac
  printf '%s\n' "${found# }"
}

in_dir() {
  local dir=$1
  shift
  ( cd "$dir" && "$@" ) < /dev/null
}

run_task() {
  local kind=$1
  local name=$2
  local dir=$3
  local entry=$4
  local task=$5
  local script
  case "$kind:$task" in
    node:build) in_dir "$dir" npm run build ;;
    node:test) in_dir "$dir" npm run test ;;
    node:e2e) in_dir "$dir" npm run test:e2e ;;
    node:lint)
      for script in lint lint:css typecheck; do
        if has_item "$entry" "$script"; then
          in_dir "$dir" npm run "$script"
        fi
      done
      ;;
    rust:build) cargo build -p "$name" ;;
    rust:test) cargo test -p "$name" ;;
    rust:lint) cargo clippy -p "$name" --all-targets -- -D warnings ;;
    rust:typegen) cargo run --quiet -p "$name" --bin "export-types-$name" ;;
  esac
}

run_everywhere() {
  local task=$1
  local kind
  local name
  local dir
  local entry
  while IFS=$'\t' read -r kind name dir entry; do
    [ -z "${only_kind:-}" ] || [ "$kind" = "$only_kind" ] || continue
    [ -z "${only_package:-}" ] || [ "$name" = "$only_package" ] || continue
    case " $(tasks_of "$kind" "$name" "$entry") " in
      *" $task "*) ;;
      *) continue ;;
    esac
    printf '==> %s %s\n' "$name" "$task"
    run_task "$kind" "$name" "$dir" "$entry" "$task"
  done < "$packages"
}

run_node_build() {
  local kind
  local name
  local dir
  local entry
  while IFS=$'\t' read -r kind name dir entry; do
    [ "$kind" = node ] || continue
    has_item "$entry" build || continue
    printf '==> %s build（前置）\n' "$name"
    in_dir "$dir" npm run build
  done < "$packages"
}

task=""
only_package=""
only_kind=""
list=""

while [ $# -gt 0 ]; do
  case "$1" in
    --list)
      list=1
      shift
      ;;
    --package)
      [ $# -ge 2 ] || { usage >&2; exit 2; }
      only_package=$2
      shift 2
      ;;
    --kind)
      [ $# -ge 2 ] || { usage >&2; exit 2; }
      only_kind=$2
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      usage >&2
      exit 2
      ;;
    *)
      [ -z "$task" ] || { usage >&2; exit 2; }
      task=$1
      shift
      ;;
  esac
done

case "$only_kind" in
  '' | rust | node) ;;
  *)
    printf '未知类别：%s。用 --kind rust 或 --kind node。\n' "$only_kind" >&2
    exit 2
    ;;
esac

if [ -n "$list" ]; then
  cut -f1-4 "$packages"
  exit 0
fi

case "$task" in
  build | lint | test | e2e | typegen) ;;
  *)
    usage >&2
    exit 2
    ;;
esac

if [ -n "$only_package" ] && ! cut -f2 "$packages" | grep -qxF "$only_package"; then
  printf '未发现包 %s。用 --list 查看可用的包。\n' "$only_package" >&2
  exit 1
fi

case "$task" in
  test | e2e)
    if [ -z "$only_kind" ] || [ "$only_kind" = node ]; then
      run_node_build
    fi
    ;;
esac

run_everywhere "$task"

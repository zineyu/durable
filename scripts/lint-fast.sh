#!/usr/bin/env bash
# 快速 lane：本地连续编辑时反复运行。
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo check --workspace --all-targets
(
  cd client
  npm run lint
  npm run lint:css
  npm run typecheck
)

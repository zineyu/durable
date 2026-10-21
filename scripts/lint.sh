#!/usr/bin/env bash
# 完整门禁：快速 lane 加全部慢检查。提交前与 CI 使用这一条。
set -euo pipefail

cd "$(dirname "$0")/.."

bash scripts/lint-fast.sh

cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --lib --document-private-items
cargo machete
cargo deny check

typos
taplo fmt --check Cargo.toml crates/*/Cargo.toml deny.toml clippy.toml rustfmt.toml
actionlint
shellcheck scripts/*.sh
nixfmt --check devenv.nix

for check in scripts/check-*.sh; do
  [ -e "$check" ] || continue
  bash "$check"
done

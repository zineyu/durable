{ pkgs, ... }:

{
  packages = with pkgs; [
    actionlint
    cargo
    cargo-deny
    cargo-machete
    clippy
    jq
    nodejs
    nixfmt
    rustc
    rustfmt
    shellcheck
    taplo
    typos
  ];

  scripts.build.exec = "durable-build";
  scripts.lint.exec = "durable-lint";
  scripts.lint-fast.exec = "durable-lint-fast";
  scripts.test.exec = "durable-test";
  scripts.typegen.exec = "durable-typegen";

  scripts.durable-build.exec = ''
    set -e
    bash scripts/task.sh build
  '';

  scripts.durable-lint.exec = ''
    set -e
    bash scripts/lint.sh
  '';

  scripts.durable-lint-fast.exec = ''
    set -e
    bash scripts/lint-fast.sh
  '';

  scripts.durable-lint-package.exec = ''
    set -e
    if [ $# -ne 1 ]; then
      echo "用法：durable-lint-package <包名>" >&2
      exit 1
    fi
    bash scripts/task.sh lint --package "$1"
  '';

  scripts.durable-test.exec = ''
    set -e
    durable-lint
    bash scripts/task.sh test
    snapshot=$(mktemp -d)
    trap 'rm -rf "$snapshot"' EXIT
    cp -R apps/client/src/api/generated "$snapshot/generated"
    cp docs/api/types.md "$snapshot/types.md"
    bash scripts/typegen.sh
    diff -ru "$snapshot/generated" apps/client/src/api/generated
    diff -u "$snapshot/types.md" docs/api/types.md
  '';

  scripts.durable-e2e.exec = ''
    set -e
    if [ ! -d apps/client/node_modules ]; then
      echo "缺少 apps/client/node_modules。先运行 devenv shell -- bash -c 'cd apps/client && npm ci'。" >&2
      exit 1
    fi
    bash scripts/task.sh build
    bash scripts/task.sh e2e
  '';

  scripts.durable-typegen.exec = ''
    set -e
    bash scripts/typegen.sh
  '';

  scripts.storybook.exec = ''
    cd apps/client && npm run storybook
  '';
}

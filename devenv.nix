{ pkgs, ... }:

{
  packages = with pkgs; [
    actionlint
    cargo
    cargo-deny
    cargo-machete
    clippy
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
    cd client && npm run build && cd ..
    cargo build --workspace
  '';

  scripts.durable-lint.exec = ''
    set -e
    bash scripts/lint.sh
  '';

  scripts.durable-lint-fast.exec = ''
    set -e
    bash scripts/lint-fast.sh
  '';

  scripts.durable-test.exec = ''
    set -e
    durable-lint
    cd client && npm run test && cd ..
    cargo test --workspace
    cargo run --quiet -p agent --bin export-types-agent
    cargo run --quiet -p hub --bin export-types-hub
    git add --intent-to-add -- client/src/api/generated
    git diff --exit-code -- client/src/api/generated
  '';

  scripts.durable-e2e.exec = ''
    set -e
    if [ ! -d client/node_modules ]; then
      echo "缺少 client/node_modules。先运行 devenv shell -- bash -c 'cd client && npm ci'。" >&2
      exit 1
    fi
    durable-build
    cd client && npm run test:e2e
  '';

  scripts.durable-typegen.exec = ''
    cargo run --quiet -p agent --bin export-types-agent
    cargo run --quiet -p hub --bin export-types-hub
  '';

  scripts.storybook.exec = ''
    cd client && npm run storybook
  '';
}

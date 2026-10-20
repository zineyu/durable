{ pkgs, ... }:

{
  packages = with pkgs; [
    cargo
    rustc
    clippy
    rustfmt
    nodejs
  ];

  scripts.build.exec = "durable-build";
  scripts.test.exec = "durable-test";
  scripts.typegen.exec = "durable-typegen";

  scripts.durable-build.exec = ''
    set -e
    cd client && npm run build && cd ..
    cargo build --workspace
  '';

  scripts.durable-test.exec = ''
    set -e
    cd client && npm run lint && npm run typecheck && npm run test && cd ..
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo run --quiet -p agent --bin export-types
    cargo run --quiet -p hub --bin export-types
    git add --intent-to-add -- client/src/api/generated
    git diff --exit-code -- client/src/api/generated
  '';

  scripts.durable-typegen.exec = ''
    cargo run --quiet -p agent --bin export-types
    cargo run --quiet -p hub --bin export-types
  '';

  scripts.storybook.exec = ''
    cd client && npm run storybook
  '';
}

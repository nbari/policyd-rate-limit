default: test
  @just --list

# Test suite
test: clippy fmt
  RUN_DOCKER_TESTS=1 cargo test --all-features

# Linting
clippy:
  cargo clippy --all-targets --all-features

# Formatting check
fmt:
  cargo fmt --all -- --check

coverage:
  cargo llvm-cov --all-features --workspace

deb:
  cargo deb

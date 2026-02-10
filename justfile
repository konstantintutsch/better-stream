# Build with release profile
[group("dev")]
build:
    cargo build --release

# Run tests
[group("dev")]
test: build
    cargo test

# Run with debug logging
[group("dev")]
run:
    RUST_LOG=better_stream=debug cargo run

CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --workspace --all-targets --locked -- -D warnings > gate-clippy.log 2>&1
cargo run -q --locked -p delphi -- lint --repo . > gate-lint.log 2>&1
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked --no-fail-fast > gate-test.log 2>&1
echo done > gate-done.log

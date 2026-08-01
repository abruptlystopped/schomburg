# Install

Current alpha support is macOS development from this repository. Install Rust with `rustup`, then verify `rustc --version` and `cargo --version`.

```sh
git clone <repository-url> schomburg
cd schomburg
cargo build --workspace
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

There is no permanent installer or formal MSRV. The Rust crates are internal alpha components. `schomburg@0.0.1` is only an npm bootstrap placeholder; it does not install or run the Rust agent.

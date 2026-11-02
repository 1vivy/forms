# Working in forms

Read [README.md](README.md) for the crate map and the publishing contract.

## Commands

```sh
cargo fmt --check
cargo clippy --workspace --exclude ratatui-gop --all-targets -- -D warnings
cargo test --workspace --exclude ratatui-gop
cargo clippy --target aarch64-unknown-uefi -p ratatui-gop -p hii-uefi -- -D warnings
cargo clippy --target x86_64-unknown-uefi -p ratatui-gop -p hii-uefi -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --exclude ratatui-gop --exclude hii-uefi --no-deps
```

Cargo writes to `out/cargo` (`.cargo/config.toml`).

## Crate boundaries

| Crate | Rule |
| --- | --- |
| `forms` | `core` + `alloc` only. No firmware, no HII, no ratatui. |
| `forms-tui` | Depends on `forms`, `ratatui-core` and `ratatui-widgets` only. |
| `ratatui-gop` | UEFI only. Must not depend on `forms`. |
| `hii-abi` | Constants and layouts only. No dependencies. |
| `hii-ifr` | Codec and configuration strings. No firmware calls. |
| `hii-uefi` | Firmware calls through `uefi-raw`. No application loop, no drawing. |

The engine (`forms`, `forms-tui`) never depends on a `hii-*` crate. Keep model
types in `forms`; keep firmware calls out of the engine.

## Rust version

`rust-toolchain.toml` pins 1.95.0 stable. `odp-platform-qcom` links `forms`,
`hii-abi` and `hii-ifr` into Patina firmware on the same pin, so do not raise
it alone and do not add a feature gate. An application that needs `std` on UEFI
(the unstable `uefi_std` gate) pins its own toolchain in its own repository.

## Rules

- Every public item has rustdoc; rustdoc warnings are errors.
- `no_std` crates use `extern crate alloc;` and import from `alloc`.
- Tests check behaviour: session transitions, codec round trips, decoding of a
  foreign compiled formset, configuration-string parsing, boundary refusals. Do
  not pin constants that the specification owns.
- Decoding foreign IFR never panics. An unknown opcode is skipped by its scope;
  an unknown expression becomes `Expr::Unsupported`.
- Write prose in short, plain sentences (ASD-STE100 style).

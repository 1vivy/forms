# Working in forms

Read [README.md](README.md) for the crate map and the publishing contract.

## Commands

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Cargo writes to `out/cargo` (`.cargo/config.toml`).

## Crate boundaries

| Item | Rule |
| --- | --- |
| `forms` model | `core` + `alloc` only. No firmware, no HII. |
| `forms::Session` | `core` + `alloc` only. Turns inputs into provider calls and views. Draws nothing. |
| `forms::widgets` | Adds `ratatui-core` and `ratatui-widgets` only. Holds no session state and no firmware call. |

The engine never depends on a `hii-*` crate; it names no firmware type. Keep
model types in the crate root and drawing in `widgets`.

## Rust version

`rust-toolchain.toml` pins 1.95.0 stable. `odp-platform-qcom` links `forms`
into Patina firmware on the same pin, so do not raise it alone and do not add a
feature gate. An application that needs `std` on UEFI (the unstable
`uefi_std` gate) pins its own toolchain in its own repository.

## Rules

- Every public item has rustdoc; rustdoc warnings are errors.
- `no_std` crates use `extern crate alloc;` and import from `alloc`.
- Tests check behaviour: session transitions, rendered layout, boundary
  refusals. Do not pin constants that the specification owns.
- Write prose in short, plain sentences (ASD-STE100 style).

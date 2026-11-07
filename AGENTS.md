# Working in forms

Read [README.md](README.md) for the crate map and the public API.

## Commands

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
make check-uefi
```

The last command lints `hii-uefi` and `forms-uefi` for `aarch64-unknown-uefi`
and `x86_64-unknown-uefi` with `-D warnings`. It runs the two aliases
`cargo check-uefi-aarch64` and `cargo check-uefi-x86`, so a target that stops
compiling fails the gate. `make check` runs the host gate.

Cargo writes to `out/cargo` (`.cargo/config.toml`).

## Crate boundaries

| Item | Rule |
| --- | --- |
| `forms::model` | `core` + `alloc` only. No firmware, no HII type, no drawing. |
| `forms::ext` | Handlers and the registry. `core` + `alloc` only, no drawing. |
| `forms::session` | `core` + `alloc` only. Turns inputs into provider calls and views. Draws nothing. |
| `forms::modal` | `core` + `alloc` only. Holds no display code. |
| `forms::widgets` | Adds `ratatui-core` and `ratatui-widgets` only. Holds no session state and no firmware call. |
| `forms-uefi` | Display and input only. Holds no page, no HII type and no log line. |
| `hii-abi` | UEFI HII package and IFR layouts. No dependency. |
| `hii-ifr` | IFR and HII package codec, plus the HII export descriptor. Depends on `forms` and `hii-abi`. |
| `hii-uefi` | `forms::Provider` over the HII database, publication through HII Config Access, and the popup protocol over `forms::Modal`. Depends on `uefi-raw`. |

The engine never depends on a `hii-*` crate; it names no firmware type. A
front end adds a `hii-*` crate only when its host publishes HII.

## Rules

- Every public item has rustdoc; rustdoc warnings are errors.
- `no_std` crates use `extern crate alloc;` and import from `alloc`.
- A control kind is a `Handler` in a `Registry`. Built-in kinds use the same
  path, so a consumer replaces or adds a control without a fork. Do not add a
  second dispatch path for a built-in kind.
- A row value is read and written through `Store` or `Provider`. The session
  holds navigation state only.
- A validation rule refuses a value before the write. Unknown conditions are
  errors, never `false`.
- Tests check behaviour: session transitions, rendered layout, condition
  evaluation, refusal, ordered-list moves, defaults, modal defaults and export
  degradation. Do not pin constants that the specification owns.
- Write prose in short, plain sentences (ASD-STE100 style).

# forms

A setup-forms engine and its UEFI HII bindings, in Rust.

The engine describes setup pages (formsets, forms, questions, conditions),
keeps the navigation state of one front end and draws it with ratatui. The HII
bindings load and operate the same model from the UEFI HII database, so a UEFI
application can show every formset that firmware publishes, and a firmware
driver can publish its own settings in the format that every UEFI form browser
reads.

The engine has no UEFI or HII dependency. A Linux program uses it alone.

## Crates

| Crate | Layer | Content | Builds for |
| --- | --- | --- | --- |
| `forms` | engine | The model, the `Provider` trait and the navigation `Session` | `no_std` + `alloc`, any target |
| `forms-tui` | engine | `FormsWidget`: a ratatui widget that draws a session `View` as a setup page | `no_std` + `alloc`, any target |
| `ratatui-gop` | backend | A ratatui backend on `EFI_GRAPHICS_OUTPUT_PROTOCOL` with an integer-scaled bitmap font | UEFI |
| `hii-abi` | HII binding | HII package and IFR layout constants, no dependencies | `no_std`, any target |
| `hii-ifr` | HII binding | Encode and decode IFR and string packages; build and parse configuration strings | `no_std` + `alloc`, any target |
| `hii-uefi` | HII binding | `HiiProvider`: a `forms::Provider` over the HII database, Config Routing and Config Access | UEFI, `no_std` + `alloc` |

Dependency rule: the engine never depends on the HII layer, and `ratatui-gop`
does not depend on `forms`.

`forms::Session` ends every page with a `< Back` row, so a front end with only
three keys (a phone: volume up, volume down, power) can return to the previous
page; `with_back_row(false)` removes it for a front end with an Esc key.
`Session::enter(guid)` opens one formset and pins it, as `FormBrowser2.SendForm`
does: Back on its entry form ends the session instead of showing the list.

| EDK II component | Equivalent here |
| --- | --- |
| `UefiInternalFormRepresentation.h`, `UefiHii.h` | `hii-abi` |
| VfrCompile, `UefiHiiLib` opcode and configuration-string helpers | `hii-ifr` |
| SetupBrowserDxe (question values, conditions, navigation) | `forms` |
| DisplayEngineDxe | `forms-tui` |
| GraphicsConsoleDxe with the system font | `ratatui-gop` |
| The HII database client of UiApp and Device Manager | `hii-uefi` |

`ratatui-gop` checks the HII row width and height before it uses a firmware font.
If the firmware substitutes an unscaled font, the backend uses its scaled bitmap font instead.


## Publish a formset from a UEFI driver

1. Build a `forms::FormSet` with its forms, statements and variable stores.
2. Encode it with `hii_ifr::encode(&set, package_list_guid)`. The result is a
   complete HII package list: one forms package, one `en-US` strings package
   and the end package.
3. Register the package list with `EFI_HII_DATABASE_PROTOCOL.NewPackageList`
   and install `EFI_HII_CONFIG_ACCESS_PROTOCOL` on the same driver handle.

Config Access makes the questions operable:

- `ExtractConfig` returns the current values as a configuration string.
- `RouteConfig` stores new values.
- `Callback` serves questions without storage, actions, and the change
  notifications of callback questions.

`hii_ifr::config` builds and parses the configuration strings for both sides.
This repository does not yet supply a Config Access implementation for a
driver; the publishing driver implements the three functions.

Registration is additive. Every formset has its own GUID, so a platform adds its
pages next to the stock pages, and every browser lists both.

## Use on Linux

A Linux program builds `forms::FormSet` values from its own settings, implements
`forms::Provider` over its own storage (`formsets`, `read`, `write`, `action`),
feeds key strokes to a `forms::Session` and draws each `forms::View` with
`forms_tui::FormsWidget` on a ratatui terminal backend. It needs none of the
`hii-*` crates.

`cargo run -p forms-tui --example host` runs that loop in a terminal over an
in-memory provider. `--once` draws one frame and prints it.

## Build and check

Rust 1.95.0 stable (`rust-toolchain.toml`). No crate uses a feature gate,
because Patina firmware links `forms`, `hii-abi` and `hii-ifr` on the same pin.

```sh
cargo fmt --check
cargo clippy --workspace --exclude ratatui-gop --all-targets -- -D warnings
cargo test --workspace --exclude ratatui-gop
cargo clippy --target aarch64-unknown-uefi -p ratatui-gop -p hii-uefi -- -D warnings
cargo clippy --target x86_64-unknown-uefi -p ratatui-gop -p hii-uefi -- -D warnings
```

`ratatui-gop` and the firmware modules of `hii-uefi` build for UEFI targets only.

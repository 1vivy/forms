# forms

A setup-forms engine, its navigation session and its ratatui setup page, in
Rust.

The engine describes setup pages (formsets, forms, questions, conditions) and
keeps the navigation state of one front end. One widget draws that state as a
PC setup page. A front end supplies the key strokes and the ratatui backend;
this repository supplies neither firmware calls nor a console.

No crate here depends on UEFI. A Linux program uses the same crate alone, and a
UEFI front end pairs it with its own text backend (see `setup` in
`odp-platform-qcom_bootstrap`).

## Structure

| Item | Layer | Content | Builds for |
| --- | --- | --- | --- |
| `forms` | model | Formsets, forms, statements, questions, values, the `Provider` trait | `no_std` + `alloc`, any target |
| `forms::Session` | session | Navigation state: `Input` in, `Event` and `View` out | `no_std` + `alloc`, any target |
| `forms::widgets` | view | `FormsWidget` and `Theme`: draw a `View` as a setup page | `no_std` + `alloc`, any target |

The crate is one unit because the parts are useless apart: a session without a
model has nothing to navigate, and a widget without a view has nothing to draw.
The session draws nothing, and the widget holds no state.

HII bindings are not part of this repository. `SendForm()`-style browsing, the
IFR codec and the HII database client left with the browser work and return only
when an external consumer exists.

## Navigation

`forms::Session` ends every page with a `< Back` row, so a front end with only
three keys (a phone: volume up, volume down, power) can return to the previous
page; `with_back_row(false)` removes it for a front end with an Esc key. Up and
Down wrap at both ends of the selectable rows. A choice editor ends with the
same `< Back` row and wraps the same way, so Enter on that row cancels the
editor and leaves the committed value unchanged.
`Session::enter(guid)` opens one formset and pins it: Back on its entry form
ends the session instead of showing the list.
`with_entry_back_row(false)` removes only the pinned entry form's Back row.
`with_readable_rows(true)` lets Up and Down select information rows, so long
information pages scroll and show their help. Enter on these rows has no effect.

Navigation and refresh preserve both the provider handle and the formset GUID.
Different providers can publish the same GUID without sharing question state.
`enter(guid)` selects the first matching provider and retains that identity.

## The page

`FormsWidget` keeps the usual setup-browser columns at any width. The prompt is
on the left, the value is on the right, and one blank cell separates them. A
value that does not fit its column loses its tail and ends with `~`, so clipped
text is visible as clipped. A read-only value (`RowKind::Info`) wraps into the
value column instead, and the row takes one screen line per wrapped part. The
selection still follows rows, not lines. Key hints are centered with one blank
cell at each edge, and the widget uses the longest hint that fits the width. The
page footer changes with the active editor; popups do not repeat those hints.
Rows with no value use the full width for their prompt. Headings draw a rule
to the right edge. Set `Theme::heading_rule` to `false` to omit the rule.

`Theme` holds the colours and styles of one page. A front end starts from
`Theme::default()` and changes single fields.

## Use on Linux

A Linux program builds `forms::FormSet` values from its own settings, implements
`forms::Provider` over its own storage (`formsets`, `read`, `write`, `action`),
feeds key strokes to a `forms::Session` and draws each `forms::View` with
`forms::widgets::FormsWidget` on a ratatui terminal backend.

`cargo run -p forms --example host` runs that loop in a terminal over an
in-memory provider. `--once` draws one frame and prints it.

Draw a frame at the width of a phone screen, and a frame with an open editor:

```sh
cargo run -p forms --example host -- --once --columns 53 --rows 20
cargo run -p forms --example host -- --once --columns 53 --rows 20 --open
```

Check three things in the printed frame:

- The key hints are centered and keep one blank cell at each edge.
- Opening an editor changes the page footer without adding hints to its border.
- A long read-only value wraps, and a clipped value ends with `~`.

## Build and check

Rust 1.95.0 stable (`rust-toolchain.toml`). No crate uses a feature gate,
because Patina firmware links `forms` at the same pin.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

`--all-targets` also covers the host example of the `forms` crate.

# forms-examples

Small programs that teach [`forms`](../forms) through code that the compiler
checks. Each program is an example target of this crate, so `cargo build
--workspace --all-targets` builds it, and every run needs no firmware.

The crate is a teaching companion. It is not a library for a product, and it
does not replace the documentation of the engine. A consumer copies the shape
of these files and keeps its own settings.

## Programs

| Example | Path | Lesson | Run |
| --- | --- | --- | --- |
| settings | `examples/settings.rs` | One template for the page and for the provider, over typed settings | `cargo run -p forms-examples --example settings` |
| popup | `examples/popup.rs` | A modal confirm over the running page, through `Session::open_popup` | `cargo run -p forms-examples --example popup` |
| actions | `examples/actions.rs` | Reset, submit and exit, as `ActionRequest` values | `cargo run -p forms-examples --example actions` |

Each program takes three modes:

- No argument: the scripted demonstration. It prints the events, the requests
  and the settings records. This mode needs no terminal, so a gate or a CI job
  can run it.
- `--once`: one frame, drawn on a test backend and printed as text. Add
  `--columns N` and `--rows N` for another size.
- `--interactive`: the page in the terminal. Keys are the keys of the host
  example: arrows, Enter, Esc and text.

```sh
cargo run -p forms-examples --example settings -- --once --columns 60 --rows 18
cargo run -p forms-examples --example popup -- --interactive
```

The shared host code, which maps keys and prints a frame, is in
`examples/support/mod.rs`. A directory under `examples` is not a cargo target
unless it holds `main.rs`, so each program includes that file with `#[path]`.

## The template

`FIELDS` and `ACTIONS` are the only authority of the example formset.
`formset()` builds the page from them, and `Settings::value` and `Settings::set`
map a question identifier to one typed field of `Settings`. One list, two uses:
the page and the provider.

The code is in `src/lib.rs`. Every item there is a doctested or documented part
of the crate, so `cargo test -p forms-examples` and `cargo doc` check it.

A new setting is one more row of `FIELDS` and one more field of `Settings`.
The page, the read and the write change together, and no front end changes.

## The provider

`SettingsProvider` implements `forms::Provider` and owns every value. The
session owns navigation only, so a front end never keeps a copy of a value: it
reads through `Session::view` and writes through `Session::input`.

The provider keeps two records:

- `draft()` is the value that the page edits. `Provider::write` changes it.
- `committed()` is the value of the last commit. `Provider::apply` copies the
  draft into it, and `Provider::discard` copies it back into the draft.

The provider records every request that it returns, and `requests()` reads that
record. A front end can log the exchange without a second authority.

`Settings::set` refuses a value that the template does not offer, and returns a
`forms::Error` with a code and a reason. The session shows the reason and keeps
the page.

## The popup

`Session::open_popup` puts a `forms::Popup` over the current page. The page
input reaches the popup first, and the person answers it there. The popup
carries a style, a type and a message, and the type fixes the legal answers.

The `popup` example shows three facts that a driver callback depends on:

- Enter accepts the highlighted answer. The first answer is highlighted, as in
  the EDK II display engine.
- Back cancels only a popup that offers Cancel. A Yes or No question cannot
  become consent, and the popup stays open.
- An answer closes the popup, leaves the page alone and returns
  `forms::Event::Popup(selection)`.

## Action requests

The `actions` example visits each action row with a draft change in place:

| Row | Request | Effect that the session applies |
| --- | --- | --- |
| Reset to defaults | `Reset` | The page stays, and the message asks for a restart. Nothing is committed. |
| Save and exit | `Submit` | `apply(FormSet)`, then the formset list. |
| Exit without saving | `Exit` | `discard(FormSet)`, then the formset list. |

A front end acts on one request only: `Reset`. It shows the message of the
page and restarts the platform. Every other request leaves the engine in the
right state before `Event::Response` arrives.

## Check

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

`--all-targets` builds the three example programs and the support module, so a
change that breaks an example fails the gate.

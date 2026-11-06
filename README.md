# forms

A setup-forms engine, its navigation session and its ratatui setup page, in
Rust.

The engine describes setup pages (formsets, forms, questions, conditions) and
keeps the navigation state of one front end. One widget draws that state as a
PC setup page. A front end supplies the key strokes and the ratatui backend.

The core crates have no firmware dependency, so a Linux program and a UEFI
image use the same engine. `forms-uefi` supplies the UEFI display and input
front end. The `hii-*` crates are optional HII adapters.

## Structure

| Item | Layer | Content | Builds for |
| --- | --- | --- | --- |
| `forms` | model | Formsets, forms, statements, questions, values, the `Provider` trait | `no_std` + `alloc`, any target |
| `forms::Session` | session | Navigation state: `Input` in, `Event` and `View` out | `no_std` + `alloc`, any target |
| `forms::widgets` | view | `FormsWidget`, `PopupWidget` and `Theme`: draw a `View` and a `Popup` as a setup page | `no_std` + `alloc`, any target |
| `forms-uefi` | front end | UEFI display and input: graphics output with a text console fallback, `KeyMap`, `Shell` | UEFI targets, host build for the key map tests |
| `hii-abi` | HII layouts | UEFI HII package and IFR layout constants, with no dependency | `no_std`, any target |
| `hii-ifr` | HII codec | IFR and HII string package codec for the forms model | `no_std` + `alloc`, any target |
| `hii-uefi` | HII provider | `forms::Provider` over the UEFI HII database, publication of a `forms::Provider` as a formset, and the HII popup protocol over `forms::Popup` | UEFI targets, host build for the producer and popup tests |
| `forms-examples` | teaching | Compiled example programs and the `Provider` behind them: a templated formset over typed settings, a popup, and action requests | host |

The crate is one unit because the parts are useless apart: a session without a
model has nothing to navigate, and a widget without a view has nothing to draw.
The session draws nothing, and the widget holds no state.

HII is an optional capability, not a foundation. `forms` and `forms-uefi` name
no HII type. A front end checks the HII database once and adds a `hii-uefi`
provider when the firmware publishes one. maOS browses the firmware formsets in
its Firmware Settings page. ODP `opq` initializes and repairs the inherited HII
stack, then shows its own pages through its native provider. A host without HII
keeps its native pages.

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

## Popups

`forms::Popup` is the modal decision that UEFI firmware publishes as
`EFI_HII_POPUP_PROTOCOL.CreatePopup`. It carries a style (`Info`, `Warning`,
`Error`), a type that fixes the legal answers (`Ok`, `OkCancel`, `YesNo`,
`YesNoCancel`) and the message. `Popup::new` selects the first answer.

`Session::open_popup` shows one over the running page, so a front end raises a
confirmation without a second session. `View::popup` carries it, and
`forms::widgets::FormsWidget` draws it as a centered modal that covers the page
rows. While a popup is open, `Input` reaches the popup first and nothing else
changes. Left and Right (and Up and Down for a three-key front end) move the
highlight and wrap; Enter accepts the highlight; the letters `Y`, `N`, `O` and
`C` accept that answer when the type offers it; Back cancels only a popup whose
type offers Cancel. A selection closes the popup and returns
`Event::Popup(selection)`. A popup never reports an answer that its type does
not offer, so a warning cannot be dismissed into consent.

## Action requests

A provider answers a change or an action with an `ActionRequest`. The value is
the number that a UEFI driver writes to `ActionRequest` in
`EFI_HII_CONFIG_ACCESS_PROTOCOL.Callback`: 0 to 9, from `None` to
`QuestionApply`. The session is the browser, so it acts on the request before
it returns `Event::Response`:

| Request | Effect |
| --- | --- |
| `None` | Nothing |
| `Reset` | The page stays, the message asks for a reset, and the front end resets. |
| `Submit` | `apply(FormSet)`, then the formset list |
| `Exit` | `discard(FormSet)`, then the formset list |
| `FormSubmitExit` | `apply(Form(..))`, then the previous page |
| `FormDiscardExit` | `discard(Form(..))`, then the previous page |
| `FormApply` | `apply(Form(..))`, the page stays |
| `FormDiscard` | `discard(Form(..))`, the page stays |
| `Reconnect` | `reconnect(handle)` |
| `QuestionApply` | `apply(Question(..))` |

A provider writes every change at once. A provider that wants a discard to
change something therefore keeps the value from before the first change of a
scope, and writes it back in `discard`. A failed call keeps the page and shows
the reason.

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
`forms::Provider` over its own storage (`formsets`, `read`, `write`, `action`)
and over the lifecycle calls (`apply`, `discard`, `reconnect`),
feeds key strokes to a `forms::Session` and draws each `forms::View` with
`forms::widgets::FormsWidget` on a ratatui terminal backend.

`cargo run -p forms --example host` runs that loop in a terminal over an
in-memory provider. `--once` draws one frame and prints it.

`crates/forms-examples` holds three compiled example programs for a consumer
that writes its own provider: a templated formset over typed settings, a popup
confirm and the handling of action requests. Each program runs on the host and
prints the events, the requests and the settings records without a terminal.
[Its README](crates/forms-examples/README.md) gives the commands and the
lessons.

Draw a frame at the width of a phone screen, and a frame with an open editor:

```sh
cargo run -p forms --example host -- --once --columns 53 --rows 20
cargo run -p forms --example host -- --once --columns 53 --rows 20 --open
```

Check three things in the printed frame:

- The key hints are centered and keep one blank cell at each edge.
- Opening an editor changes the page footer without adding hints to its border.
- A long read-only value wraps, and a clipped value ends with `~`.

## Use on UEFI

A UEFI image implements `forms::Provider` over its own facts and a
`forms_uefi::Shell`. The shell receives one description of the display and
every turn, so it can run a boot timer and act on page commands. The image
calls `forms_uefi::run` with the default keyboard map, or
`forms_uefi::run_with` with another map such as `forms_uefi::map_key_phone` on
a phone.

The front end draws on the graphics output when the firmware publishes one, and
on the inherited text console otherwise. It prefers Text Input Ex and falls
back to basic console input on the same handle. Display and input are
independent: graphics without Text Input Ex, or text without graphics output,
still works.

## Publish a formset

A driver that owns settings publishes them to the HII database, so the
firmware setup browser and every HII consumer see one standard formset. The
driver keeps its own storage.

`hii_uefi::publish::Registration::new` does five steps:

1. `hii_uefi::producer::PreparedFormSet` rewrites the formset so that every
   value question uses one buffer variable store. The store has identifier
   1, name `Forms`, the formset GUID and the declared size. Questions keep
   their form order and take sequential byte offsets.
2. The adapter encodes the package list with `hii-ifr`.
3. It installs a vendor device path on a new handle.
4. It calls `EFI_HII_DATABASE_PROTOCOL.NewPackageList` for that handle.
5. It installs `EFI_HII_CONFIG_ACCESS_PROTOCOL` on the same handle.

`ExtractConfig` reads every requested question with `Provider::read` and
builds a `<ConfigResp>`. A `NULL` request or a header-only request means every
question. `RouteConfig` parses the `<ConfigResp>`, overlays the blocks on the
current buffer, and calls `Provider::write` for each changed question. A
provider failure becomes the returned status, and `Progress` points at the
failing block.

```rust,no_run
use hii_uefi::publish::Registration;
use hii_uefi::raw::Boot;

# fn publish<P: forms::Provider + 'static>(boot: Boot, set: forms::FormSet, provider: P, handle: P::Handle)
#     -> Result<(), uefi_raw::Status> {
// SAFETY: The driver stays resident until removal succeeds.
let mut registration = unsafe { Registration::new(boot, set, provider, handle)? };
// ... use the settings ...
registration.remove()?;
# Ok(())
# }
```

The provider decides where the values live. The buffer store is only the
form view of the data. A provider that maps questions to `Opq*` settings can
publish the same formset that its native pages show.

Limits:

- The producer supports checkboxes, numbers, one-of choices and bounded text.
  Text takes `maximum * 2` bytes and stores UTF-16.
- Action questions keep no storage. `Callback` runs them through
  `Provider::action` and reports the returned request to the browser. A
  browser request to submit the formset becomes `Provider::apply`.
- Values that the provider refuses do not change the stored data, and the
  browser shows the returned status.
- The adapter uses firmware pool memory for the protocol context. A failed
  removal keeps that memory and the provider alive, because firmware would
  otherwise hold a stale pointer. Call `remove` before driver unload.

## Show a popup

A driver callback asks the setup browser for one decision through
`EFI_HII_POPUP_PROTOCOL.CreatePopup`. `hii_uefi::popup::Registration::install`
publishes that protocol, so a driver callback reaches the page that the front
end shows.

`CreatePopup` does four steps:

1. It checks the popup type and the message identifier.
2. `hii_uefi::popup::HiiStrings` resolves the message through
   `EFI_HII_STRING_PROTOCOL`.
3. It builds a `forms::Popup` and calls the `Presenter` of the front end.
4. It checks the answer against the popup type and writes the selection.

```rust,no_run
use hii_uefi::popup::{HiiStrings, Registration};
use hii_uefi::raw::Boot;

# fn serve(boot: Boot, presenter: impl hii_uefi::popup::Presenter + 'static)
#     -> Result<(), uefi_raw::Status> {
let strings = HiiStrings::new(boot)?;
// SAFETY: The driver stays resident until removal succeeds.
let mut registration = unsafe { Registration::install(boot, presenter, strings)? };
// ... the HII browser calls CreatePopup ...
registration.remove()?;
# Ok(())
# }
```

The front end implements `Presenter` over its display and input, and drives
`forms::Popup::input` for the keys. The crate holds no display code, because
display and input belong to the front end. `Registration` also accepts a
`Box<dyn Presenter>`.

Limits:

- An unknown popup type, a null HII handle, message 0, or a message that does
  not resolve returns `EFI_INVALID_PARAMETER`, and the popup does not show.
- An answer outside the popup type returns `EFI_INVALID_PARAMETER`, and the
  selection output keeps its old value.
- A popup style that the firmware does not define shows the message as
  information, as the EDK II display engine does.
- A presenter or resolver call that re-enters the protocol gets
  `EFI_ACCESS_DENIED`. The outer popup still answers.
- The context lives in firmware pool memory. A failed removal keeps that
  memory and the presenter alive. Call `remove` before driver unload.

## Build and check

Rust 1.95.0 stable (`rust-toolchain.toml`). The `forms` crate uses no feature
gate, because Patina firmware links it at the same pin.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

`--all-targets` also covers the host example of the `forms` crate and the three
example programs of `crates/forms-examples`.

`cargo build -p hii-uefi --example smoke --target x86_64-unknown-uefi` builds
a firmware smoke test. It publishes an in-memory formset, lists it with the
consumer provider, reads a value, writes a value, and checks that the provider
saw the write. Boot it in QEMU with OVMF and the serial console shows
`SMOKE PASS`. The example has the exact image and QEMU commands.

Check the front end for the UEFI targets that rustup has installed:

```sh
cargo check -p forms-uefi --target x86_64-unknown-uefi
cargo check -p forms-uefi --target aarch64-unknown-uefi
```

# forms

A setup-forms engine of its own, its navigation session, its modal model and
its ratatui setup page, in Rust.

The `forms` crate owns the page description. A *set* holds *pages*; a page
holds *rows*; a row has a stable key, a label, a control and an optional value
binding. Values live in a store that the consumer owns, or behind a provider.
No type in the crate carries a firmware identity, a firmware storage location
or a firmware opcode. HII is an adapter, not a foundation.

## Structure

| Item | Layer | Content | Builds for |
| --- | --- | --- | --- |
| `forms::model` | model | Everything a HII form set can state, in forms' own types: keys, values, expressions, controls, rows, pages, sets, defaults, `Store`, `Provider`, `Builder`, `MemoryProvider`. One file per concept | `no_std` + `alloc`, any target |
| `forms::controls` | handlers | `Handler`, `Registry`, `Decoration`, `EditState`: one dispatch path for built-in and consumer controls. `standard/` holds the HII controls, `extras/` the forms-only ones (slider, progress, divider) | `no_std` + `alloc` |
| `forms::session` | session | `Input` in, `Event` and `View` out. `navigate` (selection, links, Back), `edit` (editors, commands, validation, resets), `view` (what a renderer draws) | `no_std` + `alloc` |
| `forms::modal` | modal | `Modal`, buttons with roles, safe default, progress | `no_std` + `alloc` |
| `forms::render` | view | `FormsWidget`, `ModalWidget`, `Theme`, `KeyNames`, `Painters`, and page placement (`Margins`, `placement`) | `no_std` + `alloc` |
| `forms-uefi` | front end | UEFI display and input | UEFI targets |
| `hii-abi` | HII layouts | Raw HII package and IFR layout constants | `no_std`, any target |
| `hii-ifr` | HII codec | IFR and HII package codec, and the HII export descriptor | `no_std` + `alloc`, any target |
| `hii-uefi` | HII adapter | `forms::Provider` over the HII database, publication through HII Config Access, and the HII popup protocol over `forms::Modal` | UEFI targets, host build for the producer and popup tests |
| `forms-examples` | teaching | Compiled examples: a templated set, a modal, requests, and a consumer control with a theme override | host |
| `forms-sim` | front end | Browser front end: the real widget drawn into a panel framebuffer | host and `wasm32-unknown-unknown` |

## The model

```rust
use forms::{Builder, Control, Choice, Key};

let mut builder = Builder::new(Key(1), "Platform");
builder.page("Boot");
builder.row("Secure boot", Control::Toggle).set_binding(Key(10));
builder.row("Timeout", Control::Slider {
    minimum: 0,
    maximum: 10,
    step: 1,
    unit: String::from("s"),
    labels: vec![Choice::new("Forever", 0)],
}).set_binding(Key(11));
builder.defaults(Key(40), "Factory", [(Key(10), forms::Value::Bool(false))]);
let set = builder.build();
```

`Builder` allocates page, row and value keys in creation order, so a rebuilt set
is identical. A consumer that owns persistent state keeps its own keys and
binds them with `Row::set_binding`, or with a typed `FieldBinding<T>`.

Controls:

| Control | Meaning |
| --- | --- |
| `Heading` | A section heading |
| `Text` | An information row, live from its binding or static |
| `Link` | A page or another set, with an optional target row |
| `Action` | A provider command |
| `Toggle` | A logical switch |
| `Choice` | One of listed values |
| `Number` | A bounded number with a step |
| `Slider` | A bounded number that two keys adjust, with a unit and special labels |
| `TextInput` | Text with full keyboard editing, optionally secret |
| `OrderedList` | Chosen values in order, with reordering and a limit |
| `Reset` | Write one named default set |
| `Progress` | A progress bar or a busy indicator |
| `Divider` | A rule |
| `Unsupported` | An imported control that the crate cannot show |
| `Custom` | A registered consumer control |

`Row` carries an optional `hidden`, `disabled` and `read_only` expression, a
`locked` flag, `validation` rules (`Warning`, `Edit`, `Submit`), a `refresh`
policy and presentation hints. Conditions use a small expression language:
`Equal`, `Less`, `LessEqual`, `In`, `And`, `Or`, `Not`. A condition that cannot
be evaluated is an error, not `false`, so a page never silently enables a row.

## The session

`Session::input(Input, &mut provider) -> Event` and
`Session::view(&mut provider) -> View`. The session holds navigation state
only. It reads a value before it shows it and writes every change through the
provider, so a front end holds no copy.

`Event` is the only response vocabulary:

| Event | Meaning |
| --- | --- |
| `None` | Nothing changed |
| `Changed` | The page, editor or value changed |
| `Warning(String)` | A warning asks for one more Enter |
| `Failed(Error)` | An operation failed; the message carries the reason |
| `Response(Request)` | The session ran a provider request |
| `Modal(ModalOutcome)` | A modal finished |
| `Exit` | The person left the root page |

Validation refuses a value before any write: an `Edit` rule keeps the value
unchanged and returns `Failed`. A `Warning` rule returns `Warning` once and
accepts the confirmed input. A `Submit` rule blocks leaving the page. A
`Reset` row writes the named default set and commits it.

Every page and editor works with three keys: Up, Down and Enter, as on a
phone with volume keys and power. Up and Down reach every selectable row and
the closing Back row, so a device without a Back key can always leave a page.
An ordered list editor picks a chosen entry up with Enter, moves it with Up and
Down, and drops it with Enter; Down past the last chosen entry removes it.
Enter on an available entry adds it, and Enter on the closing `Done` entry
commits. Left and Right move an entry one place as keyboard shortcuts. A
choice editor moves its cursor with Up and Down and writes the option under
the cursor on Enter. A slider or number editor changes the value with Up and
Down and commits with Enter; a keyboard's Back leaves the old value.
`Session::with_enter_cancels_unchanged(true)` lets Enter on an unchanged value
close any value editor without a write, which gives a device without a Back
key a way out; it is off by default. `Theme::keys` names the keys in the hint
line; `KeyNames::volume_and_power()` names a phone's keys and points at the
Back row. `Theme::link_mark` marks the rows that open another page, at the
right edge.

## Extension

Every control kind, including the built-in kinds, is a `Handler` in a
`Registry`. A consumer adds a control by registering a handler for a new kind,
or replaces a built-in handler for its kind:

```rust
use forms::{Decoration, Field, Handler, Registry, Key};

struct Gauge;

impl Handler for Gauge {
    fn format(&self, _field: &Field<'_>) -> String { String::from("64%") }
    fn decoration(&self, _field: &Field<'_>) -> Decoration { Decoration::Plain }
}

let mut registry = Registry::builtin();
registry.insert(Key(0x100), Box::new(Gauge));
let session = forms::Session::<u8>::new(Vec::new()).with_registry(registry);
```

Drawing of a consumer decoration goes through a `Paint` function in
`forms::render::Painters`. The theme carries the colours, the column split,
the label limit, the overflow policy, the clip mark, the switch marks, the
alignment of a value (left or right) and a separate style for a selected row
that accepts no change. A front end starts from `Theme::default()` and changes
single fields. `forms-examples` has a working consumer control and theme
override.

`forms::render::placement` places a page inside a display. `Margins` states one
margin per edge in per-mille of the panel, so a front end can keep a different
margin on each edge; `Margins::default()` is one tenth on every edge. A
firmware display and the simulator call the same function.

## Modal

`Modal` has a title, a body, a style (`Info`, `Warning`, `Danger`, `Error`),
its own buttons with roles (`Default`, `Cancel`, `Other`), an optional progress
state and a highlighted button. The Cancel button is selected first, so one
Enter refuses a dangerous question; a modal without a Cancel button selects its
default button. `Session::open_modal` shows a modal over the running page and
`Event::Modal` returns the outcome.

## HII

`hii-ifr` converts between a set and IFR packages. The HII facts that the
model does not hold live in `hii_ifr::Export`: the form set GUID, class GUIDs,
variable stores, question identifiers and question storage, and refresh event
GUIDs. `hii_ifr::descriptor` builds one for a set with a single buffer store.

Controls that IFR cannot express degrade on export:

| Control or rule | IFR form |
| --- | --- |
| `Progress` | `TEXT` with the current value |
| `Divider` | Empty `SUBTITLE` |
| `Custom` | `TEXT` with the consumer kind |
| `Slider` | `NUMERIC` with the same bounds and step |
| Event refresh | `REFRESH_ID` when the descriptor names a GUID, otherwise omitted |
| Warning, edit, submit rule | `WARNING_IF`, `INCONSISTENT_IF`, `NO_SUBMIT_IF` |
| Disabled condition | `GRAY_OUT_IF` |
| Hidden condition | `SUPPRESS_IF`; a foreign `DISABLE_IF` decodes as `hidden` |
| Locked or read-only row | `EFI_IFR_FLAG_READ_ONLY` |

The read-only round trip loses the condition. IFR has no read-only
condition, so a row with a `read_only` expression exports as a question with
the static flag and decodes as a locked row; a rebuilt page cannot restore the
expression.

The reference browser grays a statement only for `ExpressGrayOut` or the Locked
flag (`MdeModulePkg/Universal/SetupBrowserDxe/Presentation.c:326`) and does not
draw one whose result is above it (`Presentation.c:615`), so a foreign
`DISABLE_IF` becomes a hidden row. EDK2 also leaves a `DISABLE_IF` question out
of value handling (`Presentation.c:2086`, `Setup.c:5036`). A hidden row in
`forms` is removed from the page and from navigation, and the session does not
read its value while it is hidden, but a condition can still name the value and
a submit rule of the page still runs, so hiding is not the same as dropping the
value from every calculation.

An opcode that the decoder does not model becomes an inert
`Control::Unsupported` row, so a foreign row stays visible.

`hii-uefi` publishes a set through HII Config Access and serves the HII popup
protocol through `Modal`. `hii_uefi::map_request` maps a driver action request
to a `forms::Request`; `hii_uefi::popup::modal_of` builds the modal of a popup
request.

## Build and check

Rust 1.95.0 stable (`rust-toolchain.toml`).

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

`--all-targets` also covers the host example of the `forms` crate and the four
example programs of `crates/forms-examples`.

```sh
cargo run -p forms --example host -- --once --columns 53 --rows 20
cargo run -p forms-examples --example custom
```

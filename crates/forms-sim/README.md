# forms-sim

A browser front end for one `forms` session.

The crate runs a real `forms::Session` and draws its `forms::View` with
`forms::widgets::FormsWidget` through `mousefood` and `embedded-graphics`, as a
firmware front end does. The difference is the pixel target: a RAM framebuffer
of a panel, not a graphics output. The same cell, the same widget and the same
page placement therefore give the same picture.

## Purpose

Show a `forms` page in a web browser, so a person can review a page without
firmware, without an emulator and without a build of the consuming image.

## Non-goals

- The crate holds no formset, no provider and no page of its own.
- The crate runs no device work. A provider that a consumer supplies decides
  every effect.
- The crate is not a firmware component. A firmware image links `forms` and its
  own display, not this crate.

## Parts

| Item | Content |
| --- | --- |
| `Config` | The panel size, the font cell, the fewest columns and rows, and the margin |
| `Config::placement` | Where the page sits, from `forms::page::placement` |
| `Screen` | The RGB framebuffer of the panel and `Screen::draw`, which draws one view |

## Use from another crate

A consumer supplies its own `forms::FormSet` and `forms::Provider`, drives the
session, and draws each view on a `Screen`:

```rust,ignore
use forms::{Input, Session};
use forms_sim::{Config, Screen};

let mut session = Session::new(vec![(HANDLE, formset)]);
let mut provider = MyProvider::new();
let mut screen = Screen::new(Config::default());

let view = session.view(&mut provider);
screen.draw(true, &view)?;
let event = session.input(Input::Down, &mut provider);
```

The consumer sets the panel and the placement. A consumer that draws on a phone
panel keeps `Config::default()`, which is 1272 by 2772 pixels with a cell of 6 by
10, forty columns, sixteen rows and a margin of one tenth.

## The demo site

The `demo` feature adds `forms_sim::demo`, which runs the `forms-examples`
formset and exports a small C ABI for a web page:

| Function | Result |
| --- | --- |
| `open()` | Opens the page and draws the first view |
| `press(code, character)` | Applies one key stroke and draws the result |
| `pixels()`, `width()`, `height()` | The RGB pixels and the panel size |
| `text()`, `text_len()` | The last event text as UTF-8 |

Build the site:

```sh
cargo build -p forms-sim --features demo --target wasm32-unknown-unknown --release
mkdir -p out/site
cp web/index.html web/sim.js out/site/
cp out/cargo/wasm32-unknown-unknown/release/forms_sim.wasm out/site/
python3 -m http.server -d out/site 8000
```

Open `http://localhost:8000/`. The arrow keys move, Enter selects and Escape
goes back.

## Provenance

Original. The drawing path is the `mousefood` and `embedded-graphics` stack that
`forms-uefi` uses, and the page placement is `forms::page`.

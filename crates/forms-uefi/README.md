# forms-uefi

A UEFI front end for one `forms` session. It draws the session on the
firmware display and turns console key strokes into form inputs.

The crate holds no page of its own. A consumer supplies a `forms` provider and
a `Shell`, then calls `run` or `run_with`.

## Display and input

- The display is the graphics output when the firmware publishes one. A mode
  without a usable frame buffer, or no graphics output at all, falls back to
  the inherited text console.
- The page keeps the margins of its `Frame` (default one tenth of the display on
  every edge) and draws with the frame's theme. On the graphics output it uses
  the largest integer scale of a 6 by 10 font cell that keeps the frame's
  columns and rows (default 40 by 16).
- The input is `EFI_SIMPLE_TEXT_INPUT_EX_PROTOCOL` when available. The basic
  `EFI_SIMPLE_TEXT_INPUT_PROTOCOL` on the same console handle is the fallback.
  The front end opens one input source, so a key stroke arrives once.
- The display and the input are independent. A display without graphics output
  and an input without the extended protocol both work.
- A popup that the session raises draws as a centered modal through the shared
  widget. The phone map moves its highlight with the volume keys and accepts
  the highlight with the power key.

## Public API

| Item | Use |
| --- | --- |
| `run(session, provider, shell)` | Serve one session in `Frame::default()`. |
| `run_with(session, provider, shell, &frame)` | Serve one session in the consumer's `Frame`. |
| `Frame` | The page grid (`columns`, `rows`), the `margins`, the `theme`, the `key_map` and the `drawing` (`Auto` or `Blt`). A consumer changes single fields of `Frame::default()`. |
| `ask(modal, &frame, idle)` | Show one modal over an empty page and return the chosen `ButtonRole`; Cancel is highlighted first, and an `idle` that ends the wait returns `EFI_ABORTED`. |
| `Display`, `Surface` | What `Shell::opened` receives: graphics (mode and scale) or text, the page cells and the input protocol. Its `Display` text is a one-line description. |
| `Shell` | Receives `opened` once and `turn` on every turn. `damaged` reports that another owner drew over the screen, so the page is cleared and drawn again; it defaults to never. |
| `Turn` | `Continue`, `Redraw` or `End`, as the answer of one turn. |
| `KeyMap` | `fn(u16, char) -> Option<Input>`: a scan code and a character. |
| `map_key` | The default map: a keyboard, and the power key as Enter. |
| `map_key_phone` | A phone map: the volume keys move, and the right arrow and the power key select. |
| `TURN` | The pause between two turns, 10 ms. |

`Shell::opened` receives a description of the display and the input protocol.
This crate writes no log line. A consumer that needs a log records the
description itself.

## Source and license

| | |
| --- | --- |
| Display, input and turn code | `odp-platform-qcom_bootstrap` at `ede5f8928bab00fc91fa79bed9ddde5655cb7c43`, `crates/setup/src`: `console.rs`, `graphics.rs`, `input.rs`, `text.rs` and the turn loop of `lib.rs` |
| Shell hook, single input open, key presets | maOS at `003f500882f7e3f4ddb3ffa184ac9d288051a2ad`, `efi/screen/src` |
| License of the source | `MIT` for both copies. The owner holds both repositories. |
| License of this crate | `Apache-2.0`, the repository license |

The page modules of the source (`page.rs` and `page/`) do not belong to this
crate. Each consumer supplies its own `forms` pages.

## Building

The crate is a library. It builds for `x86_64-unknown-uefi` and for
`aarch64-unknown-uefi`. A host build compiles the same code, so the key map
tests run on the host. Only a UEFI image calls `run`.

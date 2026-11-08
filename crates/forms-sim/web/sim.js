// Draws one forms page from a WebAssembly module and sends key strokes back.
//
// The module exports these functions:
//
//   open() -> 1 when the page is ready
//   press(code, character) -> 1 when the page drew again
//   pixels() -> pointer to the RGB pixels of the panel
//   width(), height() -> the panel size in pixels
//   text(), text_len() -> the last event text as UTF-8
//
// The key strokes are the codes of `forms_sim::demo::input`: 0 Volume Up,
// 1 Volume Down, 2 Home, 3 Power, 4 Back, 5 Erase and 6 for the character.
// The side buttons of the phone send the same codes.

const UP = 0;
const DOWN = 1;
const HOME = 2;
const SELECT = 3;
const BACK = 4;
const ERASE = 5;
const CHARACTER = 6;

const CODES = {
  ArrowUp: UP,
  ArrowDown: DOWN,
  Home: HOME,
  Enter: SELECT,
  ArrowLeft: BACK,
  Escape: BACK,
  Backspace: ERASE,
};

/**
 * Opens one page in `canvas` and binds the keyboard.
 * @param {{wasm: string, canvas: HTMLCanvasElement, notice: HTMLElement}} options
 * @returns {Promise<{redraw: function, open: function, press: function}>} the page
 */
export async function start(options) {
  const { instance } = await WebAssembly.instantiateStreaming(fetch(options.wasm), {});
  const ex = instance.exports;
  const canvas = options.canvas;
  const context = canvas.getContext("2d");

  const blit = () => {
    const width = ex.width();
    const height = ex.height();
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    const pixels = new Uint8Array(ex.memory.buffer, ex.pixels(), width * height * 3);
    const image = context.createImageData(width, height);
    for (let at = 0, to = 0; at < pixels.length; at += 3, to += 4) {
      image.data[to] = pixels[at];
      image.data[to + 1] = pixels[at + 1];
      image.data[to + 2] = pixels[at + 2];
      image.data[to + 3] = 255;
    }
    context.putImageData(image, 0, 0);
    const length = ex.text_len();
    const bytes = new Uint8Array(ex.memory.buffer, ex.text(), length);
    options.notice.textContent = new TextDecoder().decode(bytes);
  };

  // One path for the keyboard and the side buttons of the phone.
  const send = (code, character = 0) => {
    const drew = ex.press(code, character);
    if (drew) {
      blit();
    }
    return drew !== 0;
  };

  const open = () => {
    ex.open();
    blit();
  };

  window.addEventListener("keydown", (event) => {
    const code = CODES[event.key];
    const printable =
      [...event.key].length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey;
    if (code === undefined && !printable) {
      return;
    }
    const drew =
      code === undefined ? send(CHARACTER, event.key.codePointAt(0)) : send(code);
    if (drew) {
      event.preventDefault();
    }
  });

  open();
  return { redraw: blit, open, press: (code) => send(code) };
}

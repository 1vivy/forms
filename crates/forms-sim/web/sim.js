// Draws one forms page from a WebAssembly module and sends key strokes back.
//
// The module exports these functions:
//
//   open(fixture) -> 1 when the page is ready
//   press(code, character) -> 1 when the page drew again
//   pixels() -> pointer to the RGB pixels of the panel
//   width(), height() -> the panel size in pixels
//   text(), text_len() -> the last event text as UTF-8
//
// The key strokes are the codes of `forms_sim::demo::input`: 0 Up, 1 Down,
// 2 Left, 3 Right, 4 Home, 5 Enter, 6 Back, 7 Erase and 8 for the character.

const CODES = {
  ArrowUp: 0,
  ArrowDown: 1,
  ArrowLeft: 2,
  ArrowRight: 3,
  Home: 4,
  Enter: 5,
  Escape: 6,
  Backspace: 7,
};

/**
 * Opens one page in `canvas` and binds the keyboard.
 * @param {{wasm: string, canvas: HTMLCanvasElement, notice: HTMLElement, fixture: number}} options
 * @returns {Promise<{redraw: function, open: function}>} the page
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

  const open = (fixture) => {
    ex.open(fixture);
    blit();
  };

  window.addEventListener("keydown", (event) => {
    const code = CODES[event.key];
    const character = [...event.key].length === 1 ? event.key.codePointAt(0) : 0;
    if (code === undefined && character < 32) {
      return;
    }
    const drew = code === undefined ? ex.press(8, character) : ex.press(code, 0);
    if (drew) {
      event.preventDefault();
      blit();
    }
  });

  open(options.fixture ?? 0);
  return { redraw: blit, open };
}

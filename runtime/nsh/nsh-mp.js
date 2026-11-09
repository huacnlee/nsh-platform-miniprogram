// The Mini Program's side of nsh-platform-miniprogram, in the logic layer:
// loads the app's WebAssembly module (the engine and the app, NSH's core
// in Rust), gives it what it imports, runs it when it asks (`schedule`,
// `request_frame`), applies each commit to the tree the page draws
// (`tree.js`, `setData`) and hands the user's taps back as events.
//
// `start(page)` runs the app in `page`, a `Page` whose data has `root`
// (`pages/index/index.js`). Everything the Mini Program provides comes
// through `host`, so tests run it on Node with stand-ins.

'use strict'

const { Tree } = require('./tree.js')

/** The module the runner builds, in the package (brotli-compressed). */
const WASM_PATH = '/nsh/app.wasm.br'

function utf8Decode(bytes) {
  let out = ''
  for (let i = 0; i < bytes.length; ) {
    const b = bytes[i++]
    let cp
    if (b < 0x80) cp = b
    else if (b < 0xe0) cp = ((b & 0x1f) << 6) | (bytes[i++] & 0x3f)
    else if (b < 0xf0) cp = ((b & 0x0f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f)
    else
      cp =
        ((b & 0x07) << 18) | ((bytes[i++] & 0x3f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f)
    out += String.fromCodePoint(cp)
  }
  return out
}

function utf8Encode(text) {
  const out = []
  for (const ch of text) {
    const cp = ch.codePointAt(0)
    if (cp < 0x80) out.push(cp)
    else if (cp < 0x800) out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f))
    else if (cp < 0x10000) out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f))
    else
      out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f))
  }
  return Uint8Array.from(out)
}

/** The Mini Program's APIs this runtime uses. */
function wxHost() {
  /* global wx, WXWebAssembly */
  const canvas = wx.createOffscreenCanvas({ type: '2d', width: 1, height: 1 })
  const context = canvas.getContext('2d')
  const started = Date.now()
  return {
    instantiate: (path, imports) => WXWebAssembly.instantiate(path, imports),
    measure(font, text) {
      context.font = font
      return context.measureText(text).width
    },
    device() {
      const info = wx.getWindowInfo()
      const theme = wx.getAppBaseInfo ? wx.getAppBaseInfo().theme : 'light'
      return { width: info.windowWidth, height: info.windowHeight, pixelRatio: info.pixelRatio, dark: theme === 'dark' }
    },
    onThemeChange: (f) => wx.onThemeChange && wx.onThemeChange(f),
    onResize: (f) => wx.onWindowResize && wx.onWindowResize(f),
    now: () => Date.now() - started,
    setTimeout: (f, ms) => setTimeout(f, ms),
    clearTimeout: (t) => clearTimeout(t),
    error: (message) => console.error(message),
    log: (level, message) => (level <= 2 ? console.warn(message) : console.log(message)),
  }
}

/** Seconds east of UTC of local time at `ms` (NSH's `local_offset`). */
function localOffset(ms) {
  return -new Date(ms).getTimezoneOffset() * 60
}

/**
 * Starts the app in `page`; resolves to the running app (`event`,
 * `scroll`, `stop`).
 */
function start(page, host = wxHost(), path = WASM_PATH) {
  const tree = new Tree()
  let exports = null
  let pumpTimer = null
  let frameTimer = null
  let stopped = false
  const read = (ptr, len) => utf8Decode(new Uint8Array(exports.memory.buffer, ptr, len))
  const imports = {
    nsh: {
      now_ms: () => host.now(),
      date_now_ms: () => Date.now(),
      local_offset: localOffset,
    },
    nsh_mp: {
      submit(ptr, len) {
        const data = tree.apply(JSON.parse(read(ptr, len)))
        if (Object.keys(data).length > 0) page.setData(data)
      },
      request_frame() {
        if (frameTimer != null || stopped) return
        frameTimer = host.setTimeout(() => {
          frameTimer = null
          if (!stopped) exports.nsh_mp_frame()
        }, 16)
      },
      schedule(ms) {
        if (pumpTimer != null) host.clearTimeout(pumpTimer)
        pumpTimer = null
        if (ms < 0 || stopped) return
        pumpTimer = host.setTimeout(() => {
          pumpTimer = null
          if (!stopped) exports.nsh_mp_pump()
        }, Math.max(0, Math.ceil(ms)))
      },
      report_error: (ptr, len) => host.error(read(ptr, len)),
      measure_text: (fp, fl, tp, tl) => host.measure(read(fp, fl), read(tp, tl)),
      log: (level, ptr, len) => host.log(level, read(ptr, len)),
    },
  }
  const withString = (text, f) => {
    const bytes = utf8Encode(text)
    const ptr = exports.nsh_mp_alloc(bytes.length)
    new Uint8Array(exports.memory.buffer, ptr, bytes.length).set(bytes)
    try {
      f(ptr, bytes.length)
    } finally {
      exports.nsh_mp_free(ptr, bytes.length)
    }
  }
  const device = () => {
    const d = host.device()
    return [d.width, d.height, d.pixelRatio, d.dark ? 1 : 0]
  }
  return host.instantiate(path, imports).then((result) => {
    exports = (result.instance || result).exports
    exports.nsh_mp_init()
    if (!exports.nsh_mp_start(...device())) throw new Error('the app did not start')
    const update = () => !stopped && exports.nsh_mp_device(...device())
    host.onThemeChange(update)
    host.onResize(update)
    return {
      tree,
      /** A tap on view `id`: `onPress`, `onPressIn`, `onPressOut`, `onLongPress`. */
      event(id, name) {
        if (!stopped) withString(name, (ptr, len) => exports.nsh_mp_event(id, ptr, len))
      },
      /** Scroll view `id` is at `(x, y)`. */
      scroll(id, x, y) {
        if (!stopped) exports.nsh_mp_scroll(id, x, y)
      },
      stop() {
        stopped = true
        if (pumpTimer != null) host.clearTimeout(pumpTimer)
        if (frameTimer != null) host.clearTimeout(frameTimer)
      },
    }
  })
}

module.exports = { start, utf8Decode, utf8Encode }

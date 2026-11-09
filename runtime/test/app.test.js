'use strict'

// The packaged app on Node, as the Mini Program's logic layer runs it:
// `nsh-mp.js` loads the real module (`NSH_MP_PROJECT`, a project
// `nsh package --target miniprogram` wrote of `examples/counter`), the
// page is a stand-in that keeps what `setData` sets, and taps go back as
// the `nsh-node` component sends them. Skipped without a project.

const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const zlib = require('node:zlib')

const project = process.env.NSH_MP_PROJECT

/** Sets `value` at a `setData` path (`root.c[0].t`) of `data`. */
function setPath(data, key, value) {
  const steps = key.replace(/\[(\d+)\]/g, '.$1').split('.')
  let target = data
  for (const step of steps.slice(0, -1)) target = target[step]
  target[steps[steps.length - 1]] = value
}

function fakePage() {
  const page = {
    data: { root: { id: 1, c: [] } },
    setData(update) {
      // As the Mini Program does: a structured copy per path.
      for (const [key, value] of Object.entries(update)) setPath(page.data, key, structuredClone(value))
    },
  }
  return page
}

function host(timers) {
  return {
    instantiate: (file, imports) => {
      const bytes = zlib.brotliDecompressSync(fs.readFileSync(path.join(project, file)))
      return WebAssembly.instantiate(bytes, imports)
    },
    // 8px per character: what the layout is measured with does not matter here.
    measure: (_font, text) => [...text].length * 8,
    device: () => ({ width: 375, height: 667, pixelRatio: 2, dark: false }),
    onThemeChange() {},
    onResize() {},
    now: () => performance.now(),
    setTimeout: (f, ms) => {
      const t = setTimeout(f, ms)
      timers.add(t)
      return t
    },
    clearTimeout: (t) => clearTimeout(t),
    error: (message) => assert.fail(message),
    log() {},
  }
}

/** Every text line the page shows, depth first. */
function lines(node, out = []) {
  if (node.t) for (const line of node.t.lines) out.push(line[0])
  for (const child of node.c || []) lines(child, out)
  return out
}

/** The first node whose subtree shows `text` and that takes presses. */
function pressable(node, text) {
  for (const child of node.c || []) {
    const found = pressable(child, text)
    if (found) return found
  }
  return node.p && lines(node).includes(text) ? node : null
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 50))

test('the counter renders and counts taps', { skip: !project && 'NSH_MP_PROJECT is not set' }, async () => {
  const { start } = require(path.join(project, 'nsh/nsh-mp.js'))
  const page = fakePage()
  const timers = new Set()
  const app = await start(page, host(timers))
  await settle()
  assert.ok(lines(page.data.root).includes('Count: 0'), JSON.stringify(lines(page.data.root)))
  const button = pressable(page.data.root, 'Tap')
  assert.ok(button, 'a view taking presses shows "Tap"')
  // Laid out by NSH: a frame inside the page.
  assert.ok(button.f[2] > 0 && button.f[3] > 0, JSON.stringify(button.f))
  app.event(button.id, 'onPress')
  await settle()
  assert.ok(lines(page.data.root).includes('Count: 1'), JSON.stringify(lines(page.data.root)))
  app.stop()
  for (const t of timers) clearTimeout(t)
})

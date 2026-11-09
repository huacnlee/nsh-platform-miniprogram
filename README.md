# nsh-platform-miniprogram

[中文](README.zh-CN.md)

Run [Native Shell](https://github.com/longbridge/nsh) (NSH) apps in **WeChat Mini Programs** (微信小程序).

> **A demonstration, not an official NSH platform.** This project shows how a platform is built on NSH's public
> platform extension points, outside the NSH repository. NSH does not ship, test or support it, and its author does not
> plan to maintain it long term: there is no need for Mini Programs in our own work. If you would like to take over its
> maintenance, please get in touch ([@huacnlee](https://github.com/huacnlee)). Issues and pull requests belong here, not
> in `longbridge/nsh`.

It is also the worked example for [Building a platform](https://github.com/longbridge/nsh/blob/main/website/docs/platform-extensions.md):
every part of an NSH backend (the engine, mount batches, text, input, the command line's target) appears here in a
small, complete form. Read this repository alongside that page if you want to bring NSH to another platform.

## Usage

```sh
cargo install --git https://github.com/huacnlee/nsh-platform-miniprogram nsh-platform-miniprogram-cli
rustup target add wasm32-unknown-unknown

nsh package my-app --target miniprogram --out dist   # writes a Mini Program project
nsh run my-app --target miniprogram                  # builds it and opens it in WeChat DevTools
```

`nsh` runs `nsh-platform-miniprogram` from your `PATH` for `--target miniprogram`. The project it writes opens in
[WeChat DevTools](https://developers.weixin.qq.com/miniprogram/dev/devtools/download.html) (`brew install --cask
wechatwebdevtools`); `--appid <id>` sets the project's AppID (the default is a test AppID). `nsh run` opens it through
DevTools' command line, which needs **Settings → Security → Service port** turned on.

In the app, the platform is `platform.name === "miniprogram"` (TSX) or `platform_name() == "miniprogram"` (Rust); `platform.os` is the OS of the phone WeChat runs on (`"ios"` or `"android"`).

## How it works

```
your app (Rust)        components, layout, state
nsh-engine             LocalEngine, in WebAssembly in the Mini Program's logic layer
nsh-platform-miniprogram
  src/lib.rs           Runtime: starts the engine, pumps it, sends taps, scrolls and size changes
  src/encode.rs        each MountBatch → JSON operations (["c", 2, "view"], ["f", 2, 0, 0, 100, 40], ...)
  src/text.rs          a TextEngine on the logic layer's canvas measureText; lines of styled runs broken on the engine's side
  src/effects.rs       shadows, gradients, filters, transforms, transitions as CSS
  src/anim.rs          NSH's animation graph, ticked on the Mini Program's frames; bound values as inline CSS
  src/canvas.rs        a canvas's drawing as 2D context calls, replayed by nsh/canvas.js
  src/ffi.rs           the module's exports (nsh_mp_*) and imports (nsh_mp.*, nsh.*)
runtime/
  nsh/nsh-mp.js        loads app.wasm.br (WXWebAssembly), runs timers and frames, routes events
  nsh/tree.js          the views as a tree of plain objects; each batch becomes the fewest setData paths
  components/nsh-node  a recursive component: each view absolutely positioned at the frame NSH laid out
nsh-platform-miniprogram-cli
  package / run        `$NSH build <app>`, a cdylib runner on NSH's generated `nsh_rust_app`, built for
                       wasm32 for size, imports checked, the project written with the module brotli-compressed
```

NSH lays out everything; the Mini Program only places views at their frames and draws text lines where the engine
measured them.

## Status

- Apps written in **Rust** (`nsh.json` `"rust"` and `"root"`). TSX apps need QuickJS, which this platform does not
  build for WebAssembly yet.
- Shown: views (background, corners, borders, opacity, clipping, shadows, gradients, filters, the backdrop's blur,
  transforms, style transitions and keyframes), text (spans of their own font, size, weight, color, decoration and
  spacing on a shared baseline, inline views), images by URL and the app's own, canvases (drawn in the logic layer and shown as pictures), scroll views
  (and scrolling them from the app), text inputs (single and multiline: typing, submit, focus), taps (`onPress`,
  `onPressIn` / `onPressOut`, `onLongPress`) and press states (`active:`), the native animation graph (springs,
  timings, values that follow scroll offsets), navigation stacks on the Mini Program's own pages (its navigation
  bar, back button and back gesture), modals and sheets over the top page, light and dark appearance, window
  resizes.
- Not yet: the app's font files, gestures beyond taps (drags, pinches), pull to refresh, sticky list cells, images on
  canvases, inline views aligned other than on the baseline, video, web views.

Pull requests for any of these are welcome.

## Package size

A Mini Program's main package is limited to 2 MB. The module is built with `opt-level = "z"`, fat LTO and
`panic = "abort"`, links only the app's own UI sets (`nsh_rust_app::app_ui_sets()`, without `nsh/base`'s TSX
registry) and ships brotli-compressed (`app.wasm.br`, loaded by `WXWebAssembly` from base library 2.14.0).

| App | Module | Compressed |
|---|---|---|
| `examples/counter` | 2.42 MB | 0.72 MB |
| NSH's `examples/gallery-rs` | 6.28 MB | 1.54 MB |

`package` warns when the compressed module passes 2 MB.

## Following NSH

The platform is built against a version of NSH: mount items, the text engine and events are NSH's protocol and grow
with each release. Each release of this platform names the NSH version it supports; on WebAssembly NSH's crates import
a few functions from their host (`nsh.now_ms`, `nsh.date_now_ms`, `nsh.local_offset`), and `package` fails when a
newer NSH asks for one the runtime does not provide.

Until NSH releases the platform extension points (longbridge/nsh#1393), this repository takes NSH's crates from that
branch (`[patch.crates-io]` in `Cargo.toml`).

## Development

```sh
cargo test --workspace                       # the backend (a counter on a LocalEngine) and the CLI
node --test 'runtime/test/*.test.js'         # the runtime's tree

# the whole app: package the counter, then run its real module on Node through the real runtime
cargo build -p nsh-platform-miniprogram-cli
PATH=$PWD/target/debug:$PATH nsh package examples/counter --target miniprogram --out /tmp/mp-counter
NSH_MP_PROJECT=/tmp/mp-counter node --test 'runtime/test/*.test.js'
```

`NSH_MINIPROGRAM_SOURCE=<checkout>` makes the CLI build runners on a local checkout of this repository.

## License

Apache-2.0.

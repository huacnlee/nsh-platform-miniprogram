# nsh-platform-miniprogram

[English](README.md)

在**微信小程序**中运行 [Native Shell](https://github.com/longbridge/nsh)（NSH）应用。

> **社区维护，非 NSH 官方平台。** 本项目由社区在 NSH 仓库之外、基于 NSH 公开的平台扩展能力构建和维护。NSH 不发布、
> 不测试也不支持它。问题和 PR 请提到这里，而不是 `longbridge/nsh`。

它同时也是 [构建平台](https://github.com/longbridge/nsh/blob/main/website/zh-CN/docs/platform-extensions.md) 一文的完整案例：
NSH 后端的每个部分（引擎、挂载批次、文本、输入、命令行 target）在这里都有一个小而完整的实现。想把 NSH 带到其他平台，
可以对照那篇文档阅读本仓库。

## 使用

```sh
cargo install --git https://github.com/huacnlee/nsh-platform-miniprogram nsh-platform-miniprogram-cli
rustup target add wasm32-unknown-unknown

nsh package my-app --target miniprogram --out dist   # 生成小程序项目
nsh run my-app --target miniprogram                  # 构建并在微信开发者工具中打开
```

`--target miniprogram` 时，`nsh` 会运行 `PATH` 上的 `nsh-platform-miniprogram`。生成的项目用
[微信开发者工具](https://developers.weixin.qq.com/miniprogram/dev/devtools/download.html)（`brew install --cask
wechatwebdevtools`）打开；`--appid <id>` 设置项目的 AppID（默认是测试号）。`nsh run` 通过开发者工具的命令行打开项目，
需要先开启 **设置 → 安全设置 → 服务端口**。

在应用中，平台是 `platform.os === "miniprogram"`（TSX）或 `os().name() == "miniprogram"`（Rust）。

## 工作原理

```
你的应用（Rust）         组件、布局、状态
nsh-engine             LocalEngine，以 WebAssembly 运行在小程序逻辑层
nsh-platform-miniprogram
  src/lib.rs           Runtime：启动并驱动引擎，送回点击、滚动和尺寸变化
  src/encode.rs        每个 MountBatch → JSON 操作（["c", 2, "view"]、["f", 2, 0, 0, 100, 40]……）
  src/text.rs          基于逻辑层 canvas measureText 的 TextEngine；在引擎一侧断行
  src/ffi.rs           模块的导出（nsh_mp_*）和导入（nsh_mp.*、nsh.*）
runtime/
  nsh/nsh-mp.js        加载 app.wasm.br（WXWebAssembly），运行定时器和帧，分发事件
  nsh/tree.js          视图树（普通对象）；每个批次变成最少的 setData 路径
  components/nsh-node  递归组件：每个视图绝对定位在 NSH 布局好的 frame 上
nsh-platform-miniprogram-cli
  package / run        `$NSH build <app>`，基于 NSH 生成的 `nsh_rust_app` 写出 cdylib runner，按体积优化构建到
                       wasm32，检查导入，写出项目，模块用 brotli 压缩
```

布局全部由 NSH 完成；小程序只负责把视图放在 frame 上，并把每行文本画在引擎测量的位置。

## 状态

- 支持用 **Rust** 编写的应用（`nsh.json` 的 `"rust"` 和 `"root"`）。TSX 应用需要 QuickJS，本平台暂未把它构建到 WebAssembly。
- 已支持：视图（背景、圆角、边框、不透明度、裁剪），文本（每段文本一种样式：取第一个 span 的字体、字号、字重和颜色），
  URL 图片，滚动视图，点击（`onPress`、`onPressIn` / `onPressOut`、`onLongPress`），浅色和深色外观，窗口尺寸变化。
- 尚未支持：应用自带的资源文件（字体和图片），文本输入，点击以外的手势，渲染层动画，canvas，视频，web view，
  使用系统导航栏的导航栈。

欢迎为以上任何一项提交 PR。

## 包体积

小程序主包上限 2 MB。模块以 `opt-level = "z"`、fat LTO 和 `panic = "abort"` 构建，只链接应用自己的 UI set
（`nsh_rust_app::app_ui_sets()`，不含 `nsh/base` 面向 TSX 的注册表），并以 brotli 压缩发布（`app.wasm.br`，基础库
2.14.0 起 `WXWebAssembly` 可直接加载）。

| 应用 | 模块 | 压缩后 |
|---|---|---|
| `examples/counter` | 2.42 MB | 0.72 MB |
| NSH 的 `examples/gallery-rs` | 6.28 MB | 1.54 MB |

压缩后超过 2 MB 时 `package` 会给出警告。

## 跟随 NSH

本平台针对某个 NSH 版本构建：挂载条目、文本引擎和事件是 NSH 的协议，会随每次发布扩展。本平台每次发布都会注明所支持的
NSH 版本；在 WebAssembly 中，NSH 的 crate 会从宿主导入几个函数（`nsh.now_ms`、`nsh.date_now_ms`、`nsh.local_offset`），
新版 NSH 需要运行时未提供的导入时，`package` 会报错。

在 NSH 发布平台扩展能力（longbridge/nsh#1393）之前，本仓库从该分支获取 NSH 的 crate（`Cargo.toml` 中的 `[patch.crates-io]`）。

## 开发

```sh
cargo test --workspace                       # 后端（在 LocalEngine 上运行的计数器）和 CLI
node --test 'runtime/test/*.test.js'         # 运行时的视图树

# 完整应用：打包计数器，再在 Node 上通过真实运行时运行它的真实模块
cargo build -p nsh-platform-miniprogram-cli
PATH=$PWD/target/debug:$PATH nsh package examples/counter --target miniprogram --out /tmp/mp-counter
NSH_MP_PROJECT=/tmp/mp-counter node --test 'runtime/test/*.test.js'
```

设置 `NSH_MINIPROGRAM_SOURCE=<checkout>` 后，CLI 会基于本仓库的本地 checkout 构建 runner。

## 许可证

Apache-2.0。

# Counter

An NSH app written in Rust (`src/lib.rs`; `nsh.json` names the root, `"root": "App"`), packaged as a WeChat Mini
Program:

```sh
nsh package examples/counter --target miniprogram --out /tmp/mp-counter
nsh run examples/counter --target miniprogram    # opens it in WeChat DevTools
```

The same app runs on NSH's own platforms too (`nsh run`, `cargo run`).

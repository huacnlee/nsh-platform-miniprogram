//! The Mini Program project `package` writes: the runtime (`runtime/`,
//! embedded in this program), the app's module as `nsh/app.wasm.br`, and the
//! files WeChat DevTools reads (`app.json`, `project.config.json`), named
//! from the app's `nsh.json`.

use std::{fs, path::Path};

use anyhow::Context as _;
use serde_json::json;

/// The runtime's files, by their path in the project.
const RUNTIME: &[(&str, &str)] = &[
    ("app.js", include_str!("../../../runtime/app.js")),
    ("app.wxss", include_str!("../../../runtime/app.wxss")),
    (
        "nsh/nsh-mp.js",
        include_str!("../../../runtime/nsh/nsh-mp.js"),
    ),
    ("nsh/tree.js", include_str!("../../../runtime/nsh/tree.js")),
    (
        "nsh/canvas.js",
        include_str!("../../../runtime/nsh/canvas.js"),
    ),
    (
        "pages/index/index.js",
        include_str!("../../../runtime/pages/index/index.js"),
    ),
    (
        "pages/index/index.json",
        include_str!("../../../runtime/pages/index/index.json"),
    ),
    (
        "pages/index/index.wxml",
        include_str!("../../../runtime/pages/index/index.wxml"),
    ),
    (
        "pages/index/index.wxss",
        include_str!("../../../runtime/pages/index/index.wxss"),
    ),
    (
        "components/nsh-node/index.js",
        include_str!("../../../runtime/components/nsh-node/index.js"),
    ),
    (
        "components/nsh-node/index.json",
        include_str!("../../../runtime/components/nsh-node/index.json"),
    ),
    (
        "components/nsh-node/index.wxml",
        include_str!("../../../runtime/components/nsh-node/index.wxml"),
    ),
    (
        "components/nsh-node/index.wxss",
        include_str!("../../../runtime/components/nsh-node/index.wxss"),
    ),
];

/// The tourist AppID WeChat DevTools previews projects without an AppID of
/// their own with.
const TOURIST_APPID: &str = "touristappid";

/// The size of a Mini Program's main package (WeChat's limit).
const MAIN_PACKAGE_LIMIT: u64 = 2 * 1024 * 1024;

/// What the project takes from the app's `nsh.json`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AppConfig {
    /// `"name"`: the navigation bar's title.
    pub(crate) name: String,
    /// `"id"`, for the project's description.
    pub(crate) id: String,
}

impl AppConfig {
    pub(crate) fn read(app: &Path) -> anyhow::Result<AppConfig> {
        let path = app.join("nsh.json");
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("{} is not JSON", path.display()))?;
        let dir_name = app
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(AppConfig {
            name: json["name"]
                .as_str()
                .map_or(dir_name.clone(), str::to_owned),
            id: json["id"].as_str().map_or(dir_name, str::to_owned),
        })
    }
}

/// Writes the project into `out`, with `module` as its WebAssembly.
pub(crate) fn write(
    out: &Path,
    config: &AppConfig,
    module: &Path,
    assets: Option<&Path>,
    appid: Option<&str>,
) -> anyhow::Result<()> {
    // A project written before keeps its AppID (DevTools writes the test
    // account's there when it imports one).
    let kept = fs::read_to_string(out.join("project.config.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|p| p["appid"].as_str().map(str::to_owned))
        .filter(|id| id != TOURIST_APPID);
    let appid = appid.map(str::to_owned).or(kept);
    for (path, text) in RUNTIME {
        let file = out.join(path);
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&file, text)?;
    }
    let app_json = json!({
        "pages": ["pages/index/index"],
        "window": {
            "navigationBarTitleText": config.name,
            "navigationBarBackgroundColor": "#ffffff",
            "navigationBarTextStyle": "black",
        },
        "sitemapLocation": "sitemap.json",
        "style": "v2",
    });
    fs::write(out.join("app.json"), pretty(&app_json))?;
    fs::write(
        out.join("sitemap.json"),
        pretty(&json!({ "rules": [{ "action": "allow", "page": "*" }] })),
    )?;
    let project = json!({
        "description": format!("{} ({}), a Native Shell app", config.name, config.id),
        "appid": appid.as_deref().unwrap_or(TOURIST_APPID),
        "projectname": config.name,
        "compileType": "miniprogram",
        // `urlCheck` off: an app's images and requests reach any host while
        // it is developed; a released Mini Program lists its domains.
        "setting": { "es6": false, "enhance": false, "minified": true, "urlCheck": false },
        "packOptions": { "ignore": [], "include": [] },
    });
    fs::write(out.join("project.config.json"), pretty(&project))?;
    if let Some(assets) = assets {
        copy_assets(assets, &out.join("assets"))?;
    }
    // Brotli-compressed: `WXWebAssembly` loads `.wasm.br` files (base
    // library 2.14.0 and later), which count at their compressed size.
    let _ = fs::remove_file(out.join("nsh/app.wasm"));
    let wasm = fs::read(module).with_context(|| format!("reading {}", module.display()))?;
    let mut compressed = Vec::new();
    let params = brotli::enc::BrotliEncoderParams {
        quality: 11,
        lgwin: 24,
        ..Default::default()
    };
    brotli::BrotliCompress(&mut wasm.as_slice(), &mut compressed, &params)?;
    fs::write(out.join("nsh/app.wasm.br"), &compressed)?;
    let size = compressed.len() as u64;
    println!(
        "• the app's module is {:.2} MB ({:.2} MB uncompressed)",
        size as f64 / 1024.0 / 1024.0,
        wasm.len() as f64 / 1024.0 / 1024.0
    );
    if size > MAIN_PACKAGE_LIMIT {
        println!(
            "  warning: WeChat limits a main package to 2 MB; uploading needs the module smaller \
             (README: \"Package size\")"
        );
    }
    Ok(())
}

/// The app's files (`asset:/…`, which images show from `/assets/…`), but
/// fonts: a Mini Program loads fonts by URL (`wx.loadFontFace`), and the
/// text engine measures with the system's (the README's "Status").
fn copy_assets(from: &Path, to: &Path) -> anyhow::Result<()> {
    let _ = fs::remove_dir_all(to);
    let mut stack = vec![from.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let font = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "ttf" | "otf" | "woff" | "woff2" | "txt"));
            if font {
                continue;
            }
            let target = to.join(path.strip_prefix(from)?);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_project_devtools_opens() {
        let dir = std::env::temp_dir().join(format!("nsh-mp-project-{}", std::process::id()));
        let app = dir.join("counter");
        fs::create_dir_all(&app).unwrap();
        fs::write(
            app.join("nsh.json"),
            r#"{ "name": "Counter", "id": "com.example.counter" }"#,
        )
        .unwrap();
        let config = AppConfig::read(&app).unwrap();
        assert_eq!(config.name, "Counter");
        let module = dir.join("app.wasm");
        fs::write(&module, b"\0asm\x01\0\0\0").unwrap();
        let out = dir.join("out");
        write(&out, &config, &module, None, None).unwrap();
        let app_json = fs::read_to_string(out.join("app.json")).unwrap();
        assert!(app_json.contains("\"navigationBarTitleText\": \"Counter\""));
        let project = fs::read_to_string(out.join("project.config.json")).unwrap();
        assert!(project.contains("\"appid\": \"touristappid\""));
        for (path, _) in RUNTIME {
            assert!(out.join(path).is_file(), "{path}");
        }
        assert!(out.join("nsh/app.wasm.br").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }
}

//! `nsh-platform-miniprogram`: the `miniprogram` target of the nsh command
//! line. `nsh package --target miniprogram` (and `run`, `dev`, `test`)
//! finds this program on `PATH` and runs it with the command and its
//! arguments, `NSH` set to the nsh executable (NSH's external target
//! protocol, its `docs/dev/platforms.md`).
//!
//! `package` turns an NSH app into a WeChat Mini Program project:
//!
//! 1. `$NSH build <app>` describes the app's Rust crates and writes NSH's
//!    generated workspace, whose crate `rust-app` (`nsh_rust_app`) has the
//!    app's UI sets and root;
//! 2. a runner crate in `.nsh/miniprogram/runner` links it with this
//!    platform's backend ([`runner`]) and is built for
//!    `wasm32-unknown-unknown`;
//! 3. the module's imports are checked against what the runtime provides
//!    ([`wasm::imports`]);
//! 4. the runtime (`runtime/`, embedded here) and the module are written
//!    out as a project WeChat DevTools opens ([`project`]).

mod project;
mod runner;
mod wasm;

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use anyhow::{Context as _, bail};

const USAGE: &str = "\
nsh-platform-miniprogram: run Native Shell apps in WeChat Mini Programs

Usage (through nsh, which finds this program on PATH):
  nsh package [APP] --target miniprogram [--out DIR] [--appid ID]
  nsh run [APP] --target miniprogram [--appid ID]

Commands:
  package   Build the app into a Mini Program project (default: .nsh/out/miniprogram)
  run, dev  Build it into .nsh/miniprogram/project and open it in WeChat DevTools
  test      Not on this platform: `nsh test` runs the app's tests headless

Options:
  -o, --out DIR    Where `package` writes the project
      --appid ID   The Mini Program's AppID (default: a tourist AppID)
  -v, --verbose    Show cargo's output
  -h, --help       Show this help
  -V, --version    Show the version";

/// What a command was asked to do.
#[derive(Debug, Default, PartialEq)]
struct Options {
    command: String,
    app: Option<PathBuf>,
    out: Option<PathBuf>,
    appid: Option<String>,
    verbose: bool,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<Options> {
    let mut options = Options::default();
    let mut args = args.into_iter().map(|a| a.to_string_lossy().into_owned());
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => options.command = "help".into(),
            "-V" | "--version" => options.command = "version".into(),
            "-v" | "--verbose" => options.verbose = true,
            "-o" | "--out" => options.out = Some(value(&arg)?.into()),
            "--appid" => options.appid = Some(value(&arg)?),
            // `nsh package`'s release flag: packages are release builds here.
            "--release" => {}
            flag if flag.starts_with('-') => {
                bail!("{flag} is not an option of the miniprogram target\n\n{USAGE}")
            }
            _ if options.command.is_empty() => options.command = arg,
            _ if options.app.is_none() => options.app = Some(arg.into()),
            _ => bail!("unexpected argument {arg}\n\n{USAGE}"),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    match run(parse(env::args_os().skip(1))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: anyhow::Result<Options>) -> anyhow::Result<()> {
    let options = options?;
    match options.command.as_str() {
        "" | "help" => println!("{USAGE}"),
        "version" => println!("nsh-platform-miniprogram {}", env!("CARGO_PKG_VERSION")),
        "package" => {
            let app = app_dir(&options)?;
            let out = match &options.out {
                Some(out) => out.clone(),
                None => app.join(".nsh/out/miniprogram"),
            };
            package(&app, &out, &options)?;
            println!("✓ wrote the Mini Program to {}", out.display());
            println!("  open it in WeChat DevTools (微信开发者工具) to preview or upload it");
        }
        "run" | "dev" => {
            let app = app_dir(&options)?;
            let out = app.join(".nsh/miniprogram/project");
            package(&app, &out, &options)?;
            open(&out)?;
        }
        "test" => bail!(
            "the miniprogram target runs no tests itself: `nsh test` runs the app's tests \
             headless, on NSH's engine, as this platform runs it"
        ),
        other => bail!("{other} is not a command of the miniprogram target\n\n{USAGE}"),
    }
    Ok(())
}

/// The app's directory: `APP` (a directory or its entry file) or the
/// current one, with its `nsh.json`.
fn app_dir(options: &Options) -> anyhow::Result<PathBuf> {
    let given = options.app.clone().unwrap_or_else(|| ".".into());
    let dir = match given.is_file() {
        true => given.parent().map(Path::to_path_buf).unwrap_or_default(),
        false => given,
    };
    let dir = if dir.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        dir
    };
    if !dir.join("nsh.json").is_file() {
        bail!("{} has no nsh.json: give the app's directory", dir.display());
    }
    dir.canonicalize()
        .with_context(|| format!("no {}", dir.display()))
}

/// The nsh executable that ran this one (`NSH`), or the one on `PATH`.
fn nsh() -> OsString {
    env::var_os("NSH").unwrap_or_else(|| "nsh".into())
}

fn package(app: &Path, out: &Path, options: &Options) -> anyhow::Result<()> {
    let config = project::AppConfig::read(app)?;
    // NSH writes its generated workspace, with the app's root (`rust-app`).
    let status = Command::new(nsh())
        .arg("build")
        .arg(app)
        .arg("--out")
        .arg(app.join(".nsh/miniprogram/build"))
        .status()
        .context("running nsh (set NSH or put nsh on PATH)")?;
    if !status.success() {
        bail!("nsh build failed");
    }
    let rust_app = app.join(".nsh/rust/rust-app");
    if !rust_app.join("Cargo.toml").is_file() {
        bail!(
            "{} is not a Rust app: this platform runs apps whose root is a Rust component \
             (nsh.json \"rust\" and \"root\") so far; see the README's \"Status\"",
            app.display()
        );
    }
    let runner = app.join(".nsh/miniprogram/runner");
    runner::write(&runner, &rust_app)?;
    let module = runner::build(&runner, options.verbose)?;
    wasm::check(&wasm::imports(&std::fs::read(&module)?)?)?;
    project::write(out, &config, &module, options.appid.as_deref())
}

/// Opens the project in WeChat DevTools when its command line is there.
fn open(project: &Path) -> anyhow::Result<()> {
    let cli = [
        "/Applications/wechatwebdevtools.app/Contents/MacOS/cli",
        r"C:\Program Files (x86)\Tencent\微信web开发者工具\cli.bat",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_file());
    let Some(cli) = cli else {
        println!("✓ wrote the Mini Program to {}", project.display());
        println!("  open that directory in WeChat DevTools (微信开发者工具)");
        return Ok(());
    };
    let status = Command::new(&cli)
        .args(["open", "--project"])
        .arg(project)
        .status()
        .with_context(|| format!("running {}", cli.display()))?;
    if !status.success() {
        bail!(
            "WeChat DevTools could not open {} (turn on its service port in Settings → Security)",
            project.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<OsString> {
        line.split_whitespace().map(OsString::from).collect()
    }

    #[test]
    fn nsh_s_arguments() {
        let options = parse(args("-v package examples/counter --out dist --appid wx123")).unwrap();
        assert_eq!(
            options,
            Options {
                command: "package".into(),
                app: Some("examples/counter".into()),
                out: Some("dist".into()),
                appid: Some("wx123".into()),
                verbose: true,
            }
        );
        assert!(parse(args("package --sign")).is_err());
        assert_eq!(parse(args("run")).unwrap().command, "run");
    }
}

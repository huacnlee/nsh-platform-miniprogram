//! What a WebAssembly module imports, read from its import section, and
//! the check that the runtime (`runtime/nsh/nsh-mp.js`) provides each: a
//! module built against a newer NSH may ask its host for more, and a
//! missing import only fails when the Mini Program loads it.

use anyhow::{Context as _, bail};

/// What `nsh-mp.js` provides, by module and name.
pub(crate) const PROVIDED: &[(&str, &str)] = &[
    ("nsh", "now_ms"),
    ("nsh", "date_now_ms"),
    ("nsh", "local_offset"),
    ("nsh_mp", "submit"),
    ("nsh_mp", "request_frame"),
    ("nsh_mp", "schedule"),
    ("nsh_mp", "report_error"),
    ("nsh_mp", "measure_text"),
    ("nsh_mp", "log"),
];

fn leb(bytes: &[u8], at: &mut usize) -> anyhow::Result<u32> {
    let mut value = 0u32;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*at).context("truncated module")?;
        *at += 1;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift > 28 {
            bail!("malformed module");
        }
    }
}

fn name(bytes: &[u8], at: &mut usize) -> anyhow::Result<String> {
    let len = leb(bytes, at)? as usize;
    let s = bytes.get(*at..*at + len).context("truncated module")?;
    *at += len;
    Ok(String::from_utf8_lossy(s).into_owned())
}

/// The functions `module` imports, as `(module, name)`.
pub(crate) fn imports(module: &[u8]) -> anyhow::Result<Vec<(String, String)>> {
    if module.get(..4) != Some(b"\0asm") {
        bail!("not a WebAssembly module");
    }
    let mut at = 8;
    while at < module.len() {
        let id = module[at];
        at += 1;
        let size = leb(module, &mut at)? as usize;
        let end = at + size;
        if id == 2 {
            let mut out = Vec::new();
            let count = leb(module, &mut at)?;
            for _ in 0..count {
                let m = name(module, &mut at)?;
                let n = name(module, &mut at)?;
                let kind = *module.get(at).context("truncated module")?;
                at += 1;
                match kind {
                    // A function: its type index.
                    0 => {
                        leb(module, &mut at)?;
                        out.push((m, n));
                    }
                    // A table: element type and limits.
                    1 => {
                        at += 1;
                        limits(module, &mut at)?;
                    }
                    2 => limits(module, &mut at)?,
                    // A global: value type and mutability.
                    3 => at += 2,
                    _ => bail!("unknown import kind {kind}"),
                }
            }
            return Ok(out);
        }
        at = end;
    }
    Ok(Vec::new())
}

fn limits(bytes: &[u8], at: &mut usize) -> anyhow::Result<()> {
    let flags = *bytes.get(*at).context("truncated module")?;
    *at += 1;
    leb(bytes, at)?;
    if flags & 1 != 0 {
        leb(bytes, at)?;
    }
    Ok(())
}

/// Fails on imports the runtime does not provide.
pub(crate) fn check(imports: &[(String, String)]) -> anyhow::Result<()> {
    let missing: Vec<String> = imports
        .iter()
        .filter(|(m, n)| !PROVIDED.contains(&(m.as_str(), n.as_str())))
        .map(|(m, n)| format!("{m}.{n}"))
        .collect();
    if !missing.is_empty() {
        bail!(
            "the module imports what this platform's runtime does not provide: {} \
             (an NSH newer than this platform asks its host for more: add them to \
             runtime/nsh/nsh-mp.js and wasm.rs's PROVIDED)",
            missing.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module importing `nsh.now_ms: () -> f64` and a memory.
    const MODULE: &[u8] = &[
        0, b'a', b's', b'm', 1, 0, 0, 0, // header
        1, 5, 1, 0x60, 0, 1, 0x7c, // type section: () -> f64
        2, 27, 2, // import section, two imports
        3, b'n', b's', b'h', 6, b'n', b'o', b'w', b'_', b'm', b's', 0, 0, // func 0
        3, b'e', b'n', b'v', 3, b'm', b'e', b'm', 2, 0, 1, // memory, min 1
    ];

    #[test]
    fn reads_the_imported_functions() {
        let imports = imports(MODULE).unwrap();
        assert_eq!(imports, [("nsh".to_owned(), "now_ms".to_owned())]);
        check(&imports).unwrap();
        let unknown = [("nsh".to_owned(), "vibrate".to_owned())];
        assert!(check(&unknown).unwrap_err().to_string().contains("nsh.vibrate"));
    }
}

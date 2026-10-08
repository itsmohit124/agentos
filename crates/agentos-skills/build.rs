//! Embeds the built-in `skills/` directory of the workspace into the crate.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let skills_dir = manifest_dir.join("../../skills");
    println!("cargo:rerun-if-changed={}", skills_dir.display());

    let mut files = Vec::new();
    if skills_dir.is_dir() {
        collect(&skills_dir, &skills_dir, &mut files);
    }
    files.sort();

    let mut out = String::from(
        "/// Built-in skill files: (path relative to `skills/`, contents).\n\
         pub(crate) static FILES: &[(&str, &[u8])] = &[\n",
    );
    for (relative, absolute) in &files {
        let _ = writeln!(out, "    ({relative:?}, include_bytes!({absolute:?})),");
    }
    out.push_str("];\n");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("out dir"));
    std::fs::write(out_dir.join("builtin_skills.rs"), out).expect("write builtin_skills.rs");
}

fn collect(base: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).expect("read skills dir") {
        let path = entry.expect("skills dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(base, &path, files);
        } else {
            let relative = path
                .strip_prefix(base)
                .expect("inside skills dir")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let absolute = path.canonicalize().expect("canonicalize skill file");
            files.push((relative, absolute.display().to_string()));
        }
    }
}

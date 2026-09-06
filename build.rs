//! Embed every file under `assets/` (ADR-001). `include_str!` takes a
//! literal path and cannot glob, so this walks the directory and writes
//! `$OUT_DIR/embedded.rs` with one `(relative path, contents)` pair per file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => return,
    };
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|e| e == "toml" || e == "txt") {
            out.push(p);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    println!("cargo:rerun-if-changed=assets");
    let mut files = Vec::new();
    walk(&root, &mut files);
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded.rs");
    let mut w = fs::File::create(out).unwrap();
    writeln!(w, "/// Every asset file, as (path relative to `assets/`, contents).").unwrap();
    writeln!(w, "pub const FILES: &[(&str, &str)] = &[").unwrap();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        writeln!(w, "    ({:?}, include_str!({:?})),", rel, f.to_string_lossy()).unwrap();
    }
    writeln!(w, "];").unwrap();
}

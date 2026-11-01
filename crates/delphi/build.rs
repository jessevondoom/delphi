//! Lists the release's own specs and reference for the `about/` seed (`specs/064` §7). Markdown only, so the binary
//! does not carry the specs' images.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = String::from("pub static ABOUT: &[(&str, &[u8])] = &[\n");
    for (dir, prefix) in [("specs", "about/specs"), ("docs/reference", "about/reference")] {
        let d = root.join(dir);
        println!("cargo:rerun-if-changed={}", d.display());
        let mut names: Vec<String> = std::fs::read_dir(&d).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".md")).collect()).unwrap_or_default();
        names.sort();
        for n in names {
            let abs = d.join(&n).canonicalize().unwrap();
            out.push_str(&format!("    ({:?}, include_bytes!({:?})),\n", format!("{}/{}", prefix, n), abs.to_string_lossy()));
        }
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("about.rs");
    std::fs::write(dest, out).unwrap();
}

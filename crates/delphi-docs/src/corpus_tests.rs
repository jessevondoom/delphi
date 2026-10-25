//! A real-world check for the readers (`specs/014` §9, "Real-world"): run every file in a corpus through
//! `extract` and report what came back, without asserting on content. Ignored by default because the corpus is
//! the operator's own files.
//!
//! ```text
//! DELPHI_DOCS_CORPUS=/path/to/dir:/path/to/file.pdf cargo test -p delphi-docs real_world_corpus -- --ignored --nocapture
//! DELPHI_DOCS_SNIPPET=1 ...   # also print the first 160 characters of each result
//! ```
//!
//! The only assertion is that no file panics or hangs the reader; accuracy is judged by reading the report.

use crate::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn collect(p: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if p.is_dir() {
        if depth > 3 {
            return;
        }
        let mut kids: Vec<PathBuf> = std::fs::read_dir(p).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect()).unwrap_or_default();
        kids.sort();
        for k in kids {
            collect(&k, out, depth + 1);
        }
    } else if p.is_file() {
        out.push(p.to_path_buf());
    }
}

#[test]
#[ignore = "set DELPHI_DOCS_CORPUS to files or directories"]
fn real_world_corpus() {
    let Ok(spec) = std::env::var("DELPHI_DOCS_CORPUS") else {
        eprintln!("DELPHI_DOCS_CORPUS is not set; nothing to do");
        return;
    };
    let snippet = std::env::var("DELPHI_DOCS_SNIPPET").is_ok();
    let mut files = vec![];
    for part in spec.split(':').filter(|s| !s.is_empty()) {
        collect(Path::new(part), &mut files, 0);
    }
    let (mut ok, mut err, mut lossy) = (0, 0, 0);
    for f in &files {
        let Ok(bytes) = std::fs::read(f) else { continue };
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let t = Instant::now();
        let r = extract(&bytes, &name, &Request::default(), &Limits::default());
        let ms = t.elapsed().as_millis();
        match r {
            Ok(e) => {
                ok += 1;
                if !e.loss.is_empty() {
                    lossy += 1;
                }
                println!("OK   {:>6} ms  {:>9} B  {:<24} units {:<6} chars {:<8} loss {}  {}", ms, bytes.len(), e.format.name(), e.total_units.map(|n| n.to_string()).unwrap_or("-".into()), e.text.chars().count(), e.loss.len(), name);
                for l in &e.loss {
                    println!("       - {}", l);
                }
                if snippet {
                    println!("       > {}", e.text.chars().take(160).collect::<String>().replace('\n', " ⏎ "));
                }
            }
            Err(e) => {
                err += 1;
                println!("ERR  {:>6} ms  {:>9} B  {}  -> {}", ms, bytes.len(), name, e);
            }
        }
    }
    println!("\n{} file(s): {} read, {} refused or failed, {} with losses reported", files.len(), ok, err, lossy);
}

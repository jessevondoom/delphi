//! `delphi bundle sources add <dir> <source-id>...` (`specs/027` §2.3a, `specs/023` §2.1, `specs/055` §7; slice SK7).
//!
//! Copies library sources into a bundle's `sources/` with their licence record. Licence is information: nothing is refused
//! for it, and the confirmation happens where the bundle leaves the org (`package` for a `public` bundle, `publish`; see
//! `bundle_licence.rs`). The one refusal is a source flagged `private`. Inside an agent seat the command is refused: an
//! agent cannot export a source (`specs/055` §10).

use crate::bundle_cmd::Real;
use delphi_bundle::{Options, Report};
use delphi_engine::library::Library;
use std::path::Path;

const USAGE: &str = "usage: delphi bundle sources add <dir> <source-id>... [--replace] [--home <dir>]";

fn source_lines(r: &Report) -> Vec<String> {
    r.warns.iter().filter(|w| w.msg.starts_with("source-")).map(|w| w.msg.clone()).collect()
}

/// Turn `[sections] sources` on in `bundle.toml` (a bundle with a `sources/` directory that does not declare it fails validation).
fn declare_section(dir: &Path) -> Result<(), String> {
    let path = dir.join("bundle.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("bundle.toml: {}", e))?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("bundle.toml: {}", e))?;
    if doc.get("sections").is_none() {
        doc["sections"] = toml_edit::table();
    }
    let sections = doc["sections"].as_table_like_mut().ok_or("bundle.toml: [sections] is not a table")?;
    if sections.get("sources").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(());
    }
    sections.insert("sources", toml_edit::value(true));
    std::fs::write(&path, doc.to_string()).map_err(|e| format!("bundle.toml: {}", e))
}

pub fn add(lib: &Library, dir: &Path, ids: &[String], replace: bool) -> i32 {
    if !dir.join("bundle.toml").is_file() {
        eprintln!("delphi bundle sources add: {} has no bundle.toml (start one with `delphi bundle init`)", dir.display());
        return 2;
    }
    let mut failed = 0;
    let mut copied = 0;
    for (id, r) in lib.export_to_bundle(dir, ids, replace) {
        match r {
            Ok(e) => {
                copied += 1;
                println!("{}: copied ({}, licence {}, {} bytes of text, {} asset(s))", id, e.level.as_str(), e.licence, e.text_bytes, e.assets);
            }
            Err(why) => {
                failed += 1;
                eprintln!("{}: not copied: {}", id, why);
            }
        }
    }
    if copied > 0 {
        if let Err(e) = declare_section(dir) {
            eprintln!("delphi bundle sources add: {}", e);
            return 1;
        }
        if dir.join(".delphi-bundle/files.json").is_file() {
            match delphi_bundle::write_manifest(dir, dir.join(".claude-plugin").is_dir(), &Real) {
                Ok(h) => println!("files.json refreshed ({}); sign the bundle again if it was signed", h),
                Err(e) => eprintln!("delphi bundle sources add: files.json is stale and could not be refreshed: {}", e),
            }
        }
        let report = delphi_bundle::validate(dir, &Options::default(), &Real);
        for w in source_lines(&report) {
            println!("warning: {}", w);
        }
        match report.manifest.as_ref().map(|m| m.distribution.as_str()) {
            Some("public") => println!("this bundle is public: `package` and `publish` will list the sources that carry a licence note and ask you to confirm"),
            _ => println!("this bundle is internal: publishing it outside the org (`publish`, a mirror) asks you to confirm; `package` does not"),
        }
    }
    if failed > 0 { 1 } else { 0 }
}

pub fn cli(args: &[String], home: &Path) -> i32 {
    let ids: Vec<String> = {
        let mut skip = false;
        args.iter()
            .skip(1)
            .filter(|a| {
                if skip {
                    skip = false;
                    return false;
                }
                if matches!(a.as_str(), "--home" | "--workspace") {
                    skip = true;
                    return false;
                }
                !a.starts_with("--")
            })
            .cloned()
            .collect()
    };
    let (Some("add"), Some((dir, ids))) = (args.first().map(String::as_str), ids.split_first()) else {
        eprintln!("{}", USAGE);
        return 2;
    };
    if ids.is_empty() {
        eprintln!("{}", USAGE);
        return 2;
    }
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi bundle sources add: refused; exporting a source is the operator's act and this process was started by an agent seat");
        return 2;
    }
    add(&Library::in_home(home), Path::new(dir), ids, args.iter().any(|a| a == "--replace"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_engine::library::{AddOpts, Budgets};
    use std::fs;

    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap().flatten() {
            if e.file_type().unwrap().is_dir() { copy(&e.path(), &to.join(e.file_name())) } else { fs::copy(e.path(), to.join(e.file_name())).unwrap(); }
        }
    }

    fn put(l: &Library, d: &Path, name: &str, licence: &str, private: bool) -> String {
        let f = d.join(name);
        fs::write(&f, format!("# {}\n\nA short body about {}.\n", name, name)).unwrap();
        l.add_file(&f, &AddOpts { licence: Some(licence.into()), private, ..Default::default() }, &Budgets::default()).unwrap().id
    }

    #[test]
    fn copies_with_the_licence_record_refuses_only_private_and_the_warning_follows_distribution() {
        let home = tempfile::tempdir().unwrap();
        let l = Library::in_home(home.path());
        let work = tempfile::tempdir().unwrap();
        let restricted = put(&l, work.path(), "closed.md", "all-rights-reserved", false);
        let open = put(&l, work.path(), "open.md", "CC0-1.0", false);
        let secret = put(&l, work.path(), "secret.md", "own", true);
        let b = tempfile::tempdir().unwrap();
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/acme-platform"), b.path());
        let ids = [restricted.clone(), open.clone(), secret.clone()];
        // private is refused; the restricted source is copied (licence is information) and the exit says one failed
        assert_eq!(add(&l, b.path(), &ids, false), 1);
        assert!(!b.path().join("sources").join(&secret).exists());
        let meta: serde_json::Value = serde_json::from_slice(&fs::read(b.path().join("sources").join(&restricted).join("meta.json")).unwrap()).unwrap();
        assert_eq!(meta["licence"], "all-rights-reserved");
        assert!(meta.get("fetch_mode").is_none() && meta.get("private").is_none() && meta.get("cited_by").is_none());
        assert!(b.path().join("sources").join(&open).join("source.md").is_file());
        // an internal bundle validates clean of licence warnings; a public one warns once per restricted source, never fails
        let internal = delphi_bundle::validate(b.path(), &Options::default(), &Real);
        assert!(internal.fails.is_empty() && source_lines(&internal).iter().all(|w| !w.starts_with("source-licence")), "{:?}", internal.fails);
        let t = fs::read_to_string(b.path().join("bundle.toml")).unwrap();
        fs::write(b.path().join("bundle.toml"), t.replacen("[bundle]\n", "[bundle]\ndistribution = \"public\"\n", 1)).unwrap();
        delphi_bundle::write_manifest(b.path(), true, &Real).unwrap();
        let public = delphi_bundle::validate(b.path(), &Options::default(), &Real);
        assert!(public.fails.is_empty());
        assert_eq!(source_lines(&public).iter().filter(|w| w.starts_with("source-licence") && w.contains(&restricted)).count(), 1);
        // a second add leaves an existing source alone unless asked
        assert_eq!(add(&l, b.path(), std::slice::from_ref(&open), false), 1);
        assert_eq!(add(&l, b.path(), &[open], true), 0);
    }
}

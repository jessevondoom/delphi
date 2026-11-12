//! Channel `Xenia` of the egress enumeration (`specs/098` §12 item 16): what a publish may name. Kept in its own file because it creates
//! files in a temporary directory to stand in for a library and a workspace, and `memory_audit` (specs/050 ML-6) reads any file that
//! contains a file write beside a `local/staged` path as a possible writer of the staged zone. `egress.rs` writes nothing.

use super::*;
use crate::library::Channel;

/// Channel `Xenia`: what a publish is allowed to name. A private source of the library and the host-noted store are refused; a file that
/// is not an entry passes exactly as it did; and a record that cannot be read is private.
#[test]
fn a_publish_never_names_a_private_library_source_or_the_host_noted_store() {
    let d = tempfile::tempdir().unwrap();
    let (home, ws) = (d.path().to_path_buf(), d.path().join("workspaces/w"));
    let put = |rel: &str, body: &str| {
        let p = home.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    };
    let public = put("library/sources/pub1/versions/1/source.md", "a public page");
    put("library/sources/pub1/meta.json", r#"{"private": false, "title": "t"}"#);
    let secret = put("library/sources/sec1/source.md", "a private page");
    put("library/sources/sec1/meta.json", r#"{"private": true}"#);
    let by_licence = put("library/sources/lic1/source.md", "private by licence");
    put("library/sources/lic1/meta.json", r#"{"licence": "private"}"#);
    let broken = put("library/sources/bad1/source.md", "meta.json is not json");
    put("library/sources/bad1/meta.json", "not json");
    let no_meta = put("library/sources/nometa/source.md", "no meta.json at all");
    let claim = put("library/claims/c1.json", "{}");
    let page = put("library/pages/p1.md", "a generated page");
    let stray = put("library/sources/loose.txt", "a file directly in sources/");
    let noted = put("workspaces/w/local/staged/host-abcd1234/n1.md", "a noted entry");
    let learn = put("workspaces/w/local/staged/m_0001/n1.md", "a staged note that is not host-noted");
    let mine = put("workspaces/w/notes/mine.md", "the operator's own file");
    let outside = put("elsewhere/readme.md", "a file outside everything");
    let verdict = |f: &std::path::Path| check(&xenia_file_record(&home, Some(&ws), f), Channel::Xenia);
    // refused
    for (name, f) in [("a private source", &secret), ("a source private by licence", &by_licence), ("a source whose meta.json cannot be read", &broken), ("a source with no meta.json", &no_meta), ("a claim", &claim), ("a generated page", &page), ("a file loose in sources/", &stray), ("a host-noted entry", &noted)] {
        assert!(verdict(f).is_err(), "{name} must not be publishable");
    }
    // passes, as before
    for (name, f) in [("a public source", &public), ("a staged note that is not host-noted", &learn), ("the operator's own file", &mine), ("a file outside everything", &outside)] {
        assert!(verdict(f).is_ok(), "{name} must pass as it did");
    }
    // with no workspace the host-noted store cannot be recognised, and the library still is
    assert!(check(&xenia_file_record(&home, None, &secret), Channel::Xenia).is_err());
    // a file that does not exist is left to the publish itself to refuse
    assert!(verdict(&home.join("nope.md")).is_ok());
}

//! Spec 135 §14.11: claims on the remote. The remotes are local bare repositories; nothing is pushed anywhere else.

use super::*;

#[test]
fn a_ref_path_is_the_spec_number_and_the_slice_in_upper_case() {
    assert_eq!(ref_path("135", "g1").as_deref(), Some("135/G1"));
    assert_eq!(ref_path("135", "S1.b").as_deref(), Some("135/S1.B"));
    for bad in ["a..b", "x.", "x.lock", "§1", ".x", "-x"] {
        assert_eq!(ref_path("135", bad), None, "{}", bad);
    }
}

#[test]
fn the_machine_id_is_random_kept_and_names_no_host() {
    let t = tempfile::tempdir().unwrap();
    let a = machine_id(t.path());
    assert!(a.starts_with("m-") && a.len() == 14, "{}", a);
    assert_eq!(machine_id(t.path()), a);
    let u = tempfile::tempdir().unwrap();
    assert_ne!(machine_id(u.path()), a);
}

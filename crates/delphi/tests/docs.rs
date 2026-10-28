//! The machine-readable specs (`schemas/`) and the generated reference (`docs/reference/`) must match the program.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn schema(name: &str) -> jsonschema::Validator {
    let text = std::fs::read_to_string(root().join("schemas").join(name)).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {}", name, e));
    jsonschema::validator_for(&v).unwrap_or_else(|e| panic!("{} is not a valid schema: {}", name, e))
}

fn errors(v: &jsonschema::Validator, doc: &Value) -> Vec<String> {
    v.iter_errors(doc).map(|e| format!("{} at {}", e, e.instance_path)).collect()
}

fn must_pass(name: &str, what: &str, doc: &Value) {
    let errs = errors(&schema(name), doc);
    assert!(errs.is_empty(), "{} should satisfy {}:\n{}", what, name, errs.join("\n"));
}

fn must_fail(name: &str, what: &str, doc: &Value) {
    assert!(!schema(name).is_valid(doc), "{} should violate {}", what, name);
}

fn toml_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))).unwrap()
}

fn frontmatter(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap();
    let rest = text.strip_prefix("---\n").unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
    let yaml = rest.split("\n---").next().unwrap();
    let v: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
    serde_json::to_value(v).unwrap()
}

#[test]
fn every_schema_is_a_valid_json_schema() {
    let dir = root().join("schemas");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json") {
            schema(p.file_name().unwrap().to_str().unwrap());
            n += 1;
        }
    }
    assert!(n >= 8, "expected the schemas in {}", dir.display());
}

#[test]
fn the_example_config_satisfies_its_schema() {
    must_pass("config.schema.json", "config.example.toml", &toml_json(&root().join("crates/delphi/config.example.toml")));
}

#[test]
fn config_schema_rejects_wrong_shapes() {
    let ok = toml_json(&root().join("crates/delphi/config.example.toml"));
    let mut bad = ok.clone();
    bad["exec"]["sandbox"] = json!("maybe");
    must_fail("config.schema.json", "sandbox = maybe", &bad);
    let mut bad = ok.clone();
    bad["endpoints"] = json!([{ "id": "x", "family": "claude", "path": "telepathy", "credential": "none" }]);
    must_fail("config.schema.json", "an unknown endpoint path", &bad);
    let mut bad = ok;
    bad["harness"] = json!({ "claude": { "bridge": true, "surprise": 1 } });
    must_fail("config.schema.json", "an unknown harness key", &bad);
}

#[test]
fn the_golden_bundle_satisfies_its_schema() {
    must_pass("bundle.schema.json", "acme-platform bundle.toml", &toml_json(&root().join("dev/bundles/acme-platform/bundle.toml")));
}

#[test]
fn bundle_schema_refuses_trust_keys_and_unknown_keys() {
    let ok = toml_json(&root().join("dev/bundles/acme-platform/bundle.toml"));
    for (key, val) in [("trust", json!("high")), ("permissions", json!(["all"])), ("hooks", json!({})), ("surprise", json!(1))] {
        let mut bad = ok.clone();
        bad[key] = val;
        must_fail("bundle.schema.json", key, &bad);
    }
    let mut bad = ok.clone();
    bad["touches"]["network"] = json!(true);
    must_fail("bundle.schema.json", "touches.network = true", &bad);
    let mut bad = ok;
    bad["bundle"]["version"] = json!("one");
    must_fail("bundle.schema.json", "a non-semver version", &bad);
}

#[test]
fn every_agent_and_skill_manifest_satisfies_its_schema() {
    let mut agents = 0;
    for e in std::fs::read_dir(root().join("agents")).unwrap() {
        let p = e.unwrap().path().join("AGENT.md");
        if p.exists() {
            must_pass("agent-manifest.schema.json", &p.display().to_string(), &frontmatter(&p));
            agents += 1;
        }
    }
    let mut skills = 0;
    for e in std::fs::read_dir(root().join("skills")).unwrap() {
        let p = e.unwrap().path().join("SKILL.md");
        if p.exists() {
            must_pass("skill-manifest.schema.json", &p.display().to_string(), &frontmatter(&p));
            skills += 1;
        }
    }
    assert!(agents >= 9 && skills >= 40, "found {} agents and {} skills", agents, skills);
}

#[test]
fn events_follow_the_envelope_schema() {
    let ok = json!({ "v": 1, "seq": 3, "timestamp": "2026-10-26T10:00:00Z", "mission_id": "m1", "type": "task.started", "payload": {}, "future_field": 7 });
    must_pass("event-envelope.schema.json", "a plain event", &ok);
    let agent = json!({
        "v": 1, "seq": 4, "timestamp": "t", "mission_id": "m1", "type": "agent.usage", "payload": { "input_tokens": 1 },
        "agent": "builder", "span_id": "s", "task_id": "t1", "endpoint_id": "e", "access_path": "api", "model_tier": "fast", "model_id": "x", "attempt": 1
    });
    must_pass("event-envelope.schema.json", "an agent event", &agent);
    let mut bad = agent;
    bad.as_object_mut().unwrap().remove("span_id");
    must_fail("event-envelope.schema.json", "an agent event without span_id", &bad);
    must_fail("event-envelope.schema.json", "another major", &json!({ "v": 2, "seq": 1, "timestamp": "t", "mission_id": "m", "type": "x", "payload": {} }));
    must_fail("event-envelope.schema.json", "a non-object payload", &json!({ "v": 1, "seq": 1, "timestamp": "t", "mission_id": "m", "type": "x", "payload": [] }));
}

#[test]
fn handoffs_follow_the_envelope_schema() {
    let ok = json!({
        "handoff_id": "ho_1", "mission_id": "m1", "parent_span_id": "operator", "caller_agent": "operator", "target_agent": "builder",
        "objective": "Add retries", "context_pointers": [{ "uri": "specs/003.md#L1-L9", "reason": "the ladder" }],
        "expected_output": { "artifact_type": "answer" }, "routing_directive": { "model_tier": "workhorse", "attempt_number": 1 }
    });
    must_pass("handoff-envelope.schema.json", "a root handoff", &ok);
    let mut bad = ok.clone();
    bad["routing_directive"]["model_tier"] = json!("claude-opus");
    must_fail("handoff-envelope.schema.json", "a model ID where a tier belongs", &bad);
    let mut bad = ok;
    bad["context_pointers"][0].as_object_mut().unwrap().remove("reason");
    must_fail("handoff-envelope.schema.json", "a pointer without a reason", &bad);
}

fn memory_record() -> Value {
    json!({
        "xenia": 1, "id": "01JBX3Q8T2M9Z4K7WYH5N6D0RA", "kind": "memory", "rev": 1, "org": "acme", "store": "acme-main",
        "author": "did:key:z6MkExample", "author_role": "builder", "title": "Retry billing calls", "summary": "Billing needs idempotency keys.",
        "tags": ["billing"], "visibility": "org", "created": "2026-10-24T09:00:00Z", "updated": "2026-10-24T09:00:00Z",
        "source_ceiling": "internal_empirical", "confidence": "medium", "status": "active", "review_by": "2027-01-24",
        "pointers": [{ "scheme": "git", "locator": "https://example.com/r.git#src/a.rs", "pin": { "kind": "git-commit", "value": "abc123" }, "label": "the client", "content_type": "text/plain" }],
        "redaction": { "scanner": "rev-sec-05", "scanner_version": "1", "result": "passed", "content_hash": "blake3:00" },
        "body_kind": "none", "memory_type": "gotcha", "applies_to": { "tech": ["postgres"] }, "verified_by": "test"
    })
}

#[test]
fn xenia_records_follow_the_record_schema() {
    must_pass("xenia-record.schema.json", "a memory", &memory_record());

    let mut endorsement = memory_record();
    for k in ["memory_type", "applies_to", "verified_by", "review_by"] {
        endorsement.as_object_mut().unwrap().remove(k);
    }
    endorsement["kind"] = json!("endorsement");
    endorsement["target"] = json!("01JBX3Q8T2M9Z4K7WYH5N6D0RB@2");
    endorsement["target_hash"] = json!(format!("blake3:{}", "a".repeat(64)));
    endorsement["verdict"] = json!("dispute");
    must_fail("xenia-record.schema.json", "a dispute without a note", &endorsement);
    endorsement["note"] = json!("The summary overstates it.");
    must_pass("xenia-record.schema.json", "an endorsement", &endorsement);

    let mut tombstone = endorsement.clone();
    for k in ["target_hash", "verdict", "note"] {
        tombstone.as_object_mut().unwrap().remove(k);
    }
    tombstone["kind"] = json!("tombstone");
    tombstone["target"] = json!("01JBX3Q8T2M9Z4K7WYH5N6D0RB");
    tombstone["action"] = json!("withdraw");
    tombstone["reason"] = json!("mistake");
    must_pass("xenia-record.schema.json", "a tombstone", &tombstone);

    let mut bundle = memory_record();
    for k in ["memory_type", "applies_to", "verified_by", "review_by"] {
        bundle.as_object_mut().unwrap().remove(k);
    }
    bundle["kind"] = json!("bundle");
    bundle["bundle_name"] = json!("acme-platform");
    bundle["bundle_version"] = json!("1.4.0");
    bundle["bundle_hash"] = json!(format!("blake3:{}", "b".repeat(64)));
    bundle["tree"] = json!({ "scheme": "git", "locator": "https://git.acme.example/platform/bundle.git#.", "pin": { "kind": "git-tree", "value": "0123456789abcdef0123456789abcdef01234567" }, "label": "The bundle tree", "content_type": "other" });
    bundle["touches"] = json!({ "roles": ["strategist"], "connectors": ["slack"] });
    bundle["compat"] = json!({ "delphi": ">=0.3.0", "bundle_schema": 1 });
    must_pass("xenia-record.schema.json", "a bundle", &bundle);
    for (what, edit) in [
        ("a tree over ssh", ("locator", json!("ssh://git.acme.example/p.git#."))),
        ("a tree pinned by a branch", ("pin", json!({ "kind": "git-tree", "value": "main" }))),
        ("a tree that is not git", ("scheme", json!("https"))),
    ] {
        let mut bad = bundle.clone();
        bad["tree"][edit.0] = edit.1;
        must_fail("xenia-record.schema.json", what, &bad);
    }
    let mut bad = bundle.clone();
    bad["touches"]["roles"] = json!(["overlord"]);
    must_fail("xenia-record.schema.json", "an unknown role", &bad);
    let mut bad = bundle.clone();
    bad.as_object_mut().unwrap().remove("bundle_hash");
    must_fail("xenia-record.schema.json", "a bundle without its hash", &bad);
    let mut bad = bundle;
    bad["memory_type"] = json!("gotcha");
    must_fail("xenia-record.schema.json", "a bundle with a memory field", &bad);
}

#[test]
fn xenia_records_cannot_state_trust() {
    for key in ["tier", "trust", "trust_class", "source_type", "endorsed", "verified_by_org"] {
        let mut bad = memory_record();
        bad[key] = json!("high");
        must_fail("xenia-record.schema.json", key, &bad);
    }
    let mut bad = memory_record();
    bad["memory_type"] = json!("rumour");
    must_fail("xenia-record.schema.json", "an unknown memory_type", &bad);
    let mut bad = memory_record();
    bad["pointers"][0]["locator"] = json!("file:///etc/passwd");
    must_fail("xenia-record.schema.json", "a file: pointer", &bad);
    let mut bad = memory_record();
    bad.as_object_mut().unwrap().remove("review_by");
    must_fail("xenia-record.schema.json", "a memory without review_by", &bad);
}

#[test]
fn the_sidecar_has_exactly_its_fixed_fields() {
    let ok = json!({ "alg": "ed25519", "kid": "did:key:z6Mk", "signed_at": "2026-10-24T09:00:03Z", "roster_epoch": 7, "hash": format!("blake3:{}", "b".repeat(64)), "sig": "abc-_123" });
    must_pass("xenia-sidecar.schema.json", "a sidecar", &ok);
    let mut bad = ok.clone();
    bad["extra"] = json!(1);
    must_fail("xenia-sidecar.schema.json", "an extra field", &bad);
    let mut bad = ok;
    bad["alg"] = json!("rsa");
    must_fail("xenia-sidecar.schema.json", "another algorithm", &bad);
}

/// The command column of `delphi --help`: "delphi", "delphi chat", ... (the sentence starts after column 22).
fn help_commands() -> Vec<String> {
    delphi(&["--help"])
        .lines()
        .filter(|l| l.starts_with("  delphi") && l.len() > 24 && !l.contains("--version"))
        .map(|l| l[2..24].trim().to_string())
        .collect()
}

fn delphi(args: &[&str]) -> String {
    let t = tempfile::tempdir().unwrap();
    let out = Command::new(DELPHI).args(args).current_dir(t.path()).env("DELPHI_HOME", t.path().join("home")).output().unwrap();
    assert!(out.status.success(), "delphi {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn the_checked_in_reference_is_what_the_program_generates() {
    for (args, file) in [(&["docs", "cli"][..], "docs/reference/cli.md"), (&["docs", "man"][..], "docs/reference/delphi.1"), (&["docs", "coverage"][..], "docs/reference/harness-coverage.md")] {
        let want = delphi(args);
        let have = std::fs::read_to_string(root().join(file)).unwrap_or_default();
        assert!(want == have, "{} is out of date; regenerate it with `delphi {} > {}`", file, args.join(" "), file);
    }
}

#[test]
fn the_reference_lists_every_command_in_the_help() {
    let md = delphi(&["docs", "cli"]);
    for cmd in help_commands() {
        assert!(md.contains(&format!("### `{}`", cmd)), "{} is in --help but not in the reference", cmd);
    }
}

#[test]
fn completions_name_every_command_and_parse() {
    let cmds: Vec<String> = help_commands().iter().filter_map(|c| c.strip_prefix("delphi ")).map(String::from).collect();
    assert!(cmds.len() > 20);
    let dir = tempfile::tempdir().unwrap();
    for (shell, check) in [("bash", Some(["bash", "-n"])), ("zsh", Some(["zsh", "-n"])), ("fish", None)] {
        let script = delphi(&["completions", shell]);
        for c in &cmds {
            assert!(script.contains(c.as_str()), "{} completions lack `{}`", shell, c);
        }
        let Some([prog, flag]) = check else { continue };
        let path = dir.path().join(format!("c.{}", shell));
        std::fs::write(&path, &script).unwrap();
        match Command::new(prog).arg(flag).arg(&path).output() {
            Ok(o) => assert!(o.status.success(), "{} rejects its completions: {}", shell, String::from_utf8_lossy(&o.stderr)),
            Err(_) => eprintln!("{} is not installed; its completions were only checked for content", shell),
        }
    }
}

/// Every relative link in the user documentation points at a file that exists.
#[test]
fn documentation_links_resolve() {
    let root = root();
    let mut files: Vec<PathBuf> = ["README.md", "CHANGELOG.md", "CONTRIBUTING.md", "AGENTS.md", "llms.txt"].iter().map(|f| root.join(f)).collect();
    let mut stack = vec![root.join("docs")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "md") {
                files.push(p);
            }
        }
    }
    let mut broken = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let mut rest = text.as_str();
        while let Some(i) = rest.find("](") {
            rest = &rest[i + 2..];
            let Some(j) = rest.find(')') else { break };
            let target = &rest[..j];
            if target.is_empty() || target.contains("://") || target.starts_with('#') || target.starts_with("mailto:") || target.contains(' ') {
                continue;
            }
            let path = target.split('#').next().unwrap();
            if !f.parent().unwrap().join(path).exists() {
                broken.push(format!("{} -> {}", f.strip_prefix(&root).unwrap().display(), target));
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}

//! Spec 056 §8: the files that implement discovered assignment name no model, only attributes and classes.
//! (A repository-wide ban would fail code that predates 056; this covers the files 056 adds or extends, listed here.)

const FILES: &[&str] = &["src/models.rs", "src/model_classes.toml", "src/assign.rs", "src/grades.rs", "src/emergency.rs"];
const NAMES: &[&str] = &["opus", "sonnet", "haiku", "gpt-", "gemini-", "claude-", "llama-", "llama3", "qwen", "mistral", "o4-mini", "deepseek", "grok"];

#[test]
fn spec_056_files_name_no_model() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for f in FILES {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else { continue };
        let low = text.to_lowercase();
        for n in NAMES {
            assert!(!low.contains(n), "{f} names a model ({n}); use attributes and classes (specs/056 principle 1)");
        }
    }
}

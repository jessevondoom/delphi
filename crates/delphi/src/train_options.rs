//! The `options_considered` check (`specs/124` §7 and S12): does a deliverable hold an "Options considered" section with at least
//! three options that differ, each with a test?
//!
//! It is a structural check on text: a heading, a table, cells and words. No model reads anything. It can say that the section
//! has the shape the `divergent-options` skill asks for; it cannot say that an option is a good design, that a test would work, or
//! that two options with different words differ in mechanism. Those are for review by a person (`specs/124` §11: no
//! model-graded judgement of quality is built).
//!
//! What it reads: the first heading that says "Options considered" (any depth, any case, a section number allowed, as the lint
//! `spec-options` reads it), then the first table under it, up to the next heading of the same depth or above. A row is an option.
//! Its last cell is how the option would be tested; the cell before it is what the option is; the cells before that are its label
//! and its method. Both the skill's four-column table and a three-column one (`A, conventional | what | test`) are read. An option
//! written as a list, or as prose, is not seen: the skill prescribes a table.

use std::collections::BTreeSet;

/// What a passing section held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub options: usize,
}

/// A cell that says nothing.
const PLACEHOLDERS: [&str; 7] = ["...", "…", "tbd", "todo", "-", "n/a", "?"];
/// Two descriptions that share this share of their words (or more) are one design written twice.
const SAME_DESIGN: f64 = 0.8;

struct Row {
    /// The label and method cells, joined, lower case.
    head: String,
    what: String,
    test: String,
}

fn heading_level(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let n = t.bytes().take_while(|b| *b == b'#').count();
    (n > 0 && n <= 6).then_some(n)
}

fn cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let inner = t.strip_prefix('|')?;
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    Some(inner.split('|').map(|c| c.trim().to_string()).collect())
}

fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn says_nothing(cell: &str) -> bool {
    let c = cell.trim().trim_matches(|ch: char| matches!(ch, '*' | '_' | '`' | '<' | '>')).trim().to_lowercase();
    c.is_empty() || PLACEHOLDERS.contains(&c.as_str())
}

/// Words of four letters or more, lower case, cut to five letters (the measure the trigger tests use).
fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase().split(|c: char| !c.is_ascii_alphabetic()).filter(|w| w.len() >= 4).map(|w| w.chars().take(5).collect()).collect()
}

/// The lines of the section under the first "Options considered" heading. A code fence's marks are dropped and its lines kept: a
/// seat that answers with the section inside a fence has still written it.
fn section(text: &str) -> Option<Vec<&str>> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("```") && !l.trim_start().starts_with("~~~")).collect();
    let at = lines.iter().position(|l| heading_level(l).is_some() && l.to_lowercase().contains("options considered"))?;
    let level = heading_level(lines[at])?;
    let end = lines[at + 1..].iter().position(|l| heading_level(l).is_some_and(|n| n <= level)).map_or(lines.len(), |n| at + 1 + n);
    Some(lines[at + 1..end].to_vec())
}

/// `Ok` with what was found, else the first thing missing, in words.
pub fn grade(text: &str, min: usize) -> Result<Summary, String> {
    let body = section(text).ok_or("no heading says \"Options considered\"")?;
    // The first table of the section: the rows after its separator line, until the table ends.
    let mut rows: Vec<Row> = Vec::new();
    let mut in_table = false;
    for line in &body {
        match cells(line) {
            Some(c) if is_separator(&c) => in_table = true,
            Some(c) if in_table => {
                if c.len() < 3 {
                    return Err(format!("an option's row has {} cell(s); it needs a label, what the option is and how it would be tested", c.len()));
                }
                let (test, what) = (c[c.len() - 1].clone(), c[c.len() - 2].clone());
                rows.push(Row { head: c[..c.len() - 2].join(" ").to_lowercase(), what, test });
            }
            Some(_) => {}
            None if in_table => break,
            None => {}
        }
    }
    if rows.is_empty() {
        return Err("the section has no table of options".into());
    }
    if rows.len() < min {
        return Err(format!("{} option(s) in the table, wanted at least {}", rows.len(), min));
    }
    let name = |r: &Row| r.head.split(|c: char| !c.is_ascii_alphanumeric()).find(|w| !w.is_empty()).unwrap_or("?").to_uppercase();
    let mut labels = BTreeSet::new();
    for r in &rows {
        if says_nothing(&r.head) {
            return Err("an option has no label".into());
        }
        if !labels.insert(r.head.clone()) {
            return Err(format!("two options carry the same label and method ({})", name(r)));
        }
        if says_nothing(&r.what) {
            return Err(format!("option {} does not say what it is", name(r)));
        }
        if says_nothing(&r.test) {
            return Err(format!("option {} has no sentence on how it would be tested", name(r)));
        }
    }
    // The three kinds of the skill: one conventional, one that removes a constraint, and one more by a named method.
    let conventional = rows.iter().position(|r| r.head.contains("conventional"));
    let constraint = rows.iter().position(|r| r.head.contains("constraint"));
    match (conventional, constraint) {
        (None, _) => return Err("no option is marked conventional".into()),
        (_, None) => return Err("no option is marked as removing a constraint".into()),
        (Some(a), Some(b)) if a == b => return Err("one row is marked both conventional and constraint removed; they are two options".into()),
        (Some(_), Some(_)) if rows.len() < 3 => return Err("no option by a named method beside the conventional one and the constraint removed".into()),
        _ => {}
    }
    for (i, a) in rows.iter().enumerate() {
        for b in &rows[i + 1..] {
            let (wa, wb) = (words(&a.what), words(&b.what));
            let union = wa.union(&wb).count();
            if union == 0 || wa.intersection(&wb).count() as f64 / union as f64 >= SAME_DESIGN {
                return Err(format!("options {} and {} are described in the same words; they are one design", name(a), name(b)));
            }
        }
    }
    if !body.iter().any(|l| l.to_lowercase().contains("chosen")) {
        return Err("the section does not say which option was chosen".into());
    }
    Ok(Summary { options: rows.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    const GOOD: &str = "# Spec\n\n### Options considered\n| Option | Method | What it is | How it would be tested |\n| :-- | :-- | :-- | :-- |\n| A | conventional | A worker pool drains a queue table. | Load 10,000 jobs and count the ones lost. |\n| B | constraint removed: jobs survive a restart | Keep jobs in memory only. | Kill the daemon mid-run and count what the caller must resend. |\n| C | inversion | The client pulls its export when it is ready. | Run 100 clients and measure the daemon's idle load. |\n\nChosen: A, because losing jobs is not acceptable. Flagged by the guard: none.\n\n## Next\n";

    #[test]
    fn a_section_with_three_distinct_tested_options_passes_and_each_missing_part_is_named() {
        assert_eq!(grade(GOOD, 3), Ok(Summary { options: 3 }));
        // The same section inside a code fence, as a seat may answer, is still read.
        assert_eq!(grade(&format!("Here it is:\n```markdown\n{}```\n", GOOD), 3), Ok(Summary { options: 3 }));
        let fails = |text: String, why: &str| {
            let got = grade(&text, 3).unwrap_err();
            assert!(got.contains(why), "wanted {why:?}, got {got:?}");
        };
        fails(GOOD.replace("### Options considered", "We weighed the options considered."), "no heading");
        fails(GOOD.lines().filter(|l| !l.starts_with("| C")).collect::<Vec<_>>().join("\n"), "2 option(s)");
        fails(GOOD.replace("Run 100 clients and measure the daemon's idle load.", "..."), "option C has no sentence on how it would be tested");
        fails(GOOD.replace("| conventional |", "| the usual |"), "no option is marked conventional");
        fails(GOOD.replace("constraint removed: jobs survive a restart", "another way"), "removing a constraint");
        fails(GOOD.replace("Keep jobs in memory only.", "A worker pool drains a queue table!").replace("The client pulls its export when it is ready.", "A pool of workers drains the queue table."), "one design");
        fails(GOOD.replace("Chosen: A", "Decision: A"), "which option was chosen");
        fails(GOOD.replace("| A | conventional |", "| B | constraint removed: jobs survive a restart |"), "same label");
        // The table of the next section is not this section's.
        fails("## Options considered\nNone yet.\n## Other\n| a | b | c |\n| - | - | - |\n| A | conventional | x |\n".into(), "no table");
    }

    /// The samples kept beside the training case are graded as their `expect.toml` says: this is the check graded against known
    /// answers, with no model. A sample is a hand-written section, not a seat's output.
    #[test]
    fn the_samples_of_the_training_case_are_graded_as_their_key_says() {
        let dir = root().join("dev/train/suites/spec124/cases/options-considered/samples");
        let key: toml::Table = std::fs::read_to_string(dir.join("expect.toml")).unwrap().parse().unwrap();
        let samples = key["sample"].as_array().unwrap();
        assert!(samples.len() >= 6, "a passing sample and one for each way to fail");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".md")).collect();
        on_disk.sort();
        let mut named: Vec<String> = samples.iter().map(|s| s["file"].as_str().unwrap().to_string()).collect();
        named.sort();
        assert_eq!(on_disk, named, "every sample file is in the key and the key names no other");
        for s in samples {
            let (file, pass) = (s["file"].as_str().unwrap(), s["pass"].as_bool().unwrap());
            let got = grade(&std::fs::read_to_string(dir.join(file)).unwrap(), 3);
            assert_eq!(got.is_ok(), pass, "{file}: {got:?}");
            if let Err(e) = got {
                let why = s["why"].as_str().unwrap_or_else(|| panic!("{file}: a failing sample says why"));
                assert!(e.contains(why), "{file}: wanted {why:?}, got {e:?}");
            }
        }
    }

    /// What the check says about text it was not written beside. Spec 125 is the one spec in this repository numbered above 124
    /// (the first the rule applies to); the skill's own output format is a template of placeholders and must not pass.
    #[test]
    fn the_first_spec_under_the_rule_passes_and_the_skills_template_does_not() {
        let spec = std::fs::read_to_string(root().join("specs/125-load-sensitive-tests-made-deterministic.md")).unwrap();
        assert_eq!(grade(&spec, 3), Ok(Summary { options: 3 }));
        let skill = std::fs::read_to_string(root().join("skills/divergent-options/SKILL.md")).unwrap();
        let template = &skill[skill.find("## Output format").unwrap()..skill.find("## Done when").unwrap()];
        assert!(grade(template, 3).unwrap_err().contains("does not say what it is"), "{:?}", grade(template, 3));
    }
}

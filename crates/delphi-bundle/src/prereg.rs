//! Pre-registration files and their hash pin (`specs/027` §7.2).
//!
//! A pre-registration is committed before any confirmatory run. Its **pin** is the blake3 of the file with the body of
//! its `## Deviations` section removed: nothing else may change after the pin, and a deviation is appended under that
//! heading, so adding one must not move the hash. The same blake3 digests name the task set and the rubric inside it.

use std::path::Path;

const PREFIX: &str = "blake3:";

fn heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (hashes > 0 && line[hashes..].starts_with(' ')).then(|| (hashes, line[hashes..].trim()))
}

/// The text the pin covers: the file without the body of its `Deviations` section (the heading line stays).
pub fn pinned_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let Some((at, level)) = lines.iter().enumerate().find_map(|(i, l)| heading(l).filter(|(_, t)| t.eq_ignore_ascii_case("Deviations")).map(|(lv, _)| (i, lv))) else {
        return text.to_string();
    };
    let end = lines[at + 1..].iter().position(|l| heading(l).is_some_and(|(lv, _)| lv <= level)).map(|p| at + 1 + p).unwrap_or(lines.len());
    let mut kept: Vec<&str> = lines[..=at].to_vec();
    kept.extend_from_slice(&lines[end..]);
    kept.join("\n") + "\n"
}

/// `blake3:<hex>` of the pinned text.
pub fn pin(text: &str) -> String {
    format!("{}{}", PREFIX, blake3::hash(pinned_text(text).as_bytes()).to_hex())
}

/// `blake3:<hex>` of a file's bytes, or of a directory: the sorted lines `<relative path> <file digest>` of every regular
/// file (dotfiles included; symlinks are not followed and are an error, so a digest names exactly what is in the tree).
pub fn digest(path: &Path) -> Result<String, String> {
    if path.is_file() {
        let b = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        return Ok(format!("{}{}", PREFIX, blake3::hash(&b).to_hex()));
    }
    let mut rows = Vec::new();
    walk(path, "", &mut rows)?;
    rows.sort();
    Ok(format!("{}{}", PREFIX, blake3::hash(rows.join("\n").as_bytes()).to_hex()))
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix, name) };
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            return Err(format!("{}: a symlink is not hashed", rel));
        }
        if ft.is_dir() {
            walk(&e.path(), &rel, out)?;
        } else if ft.is_file() {
            let b = std::fs::read(e.path()).map_err(|e| format!("{}: {}", rel, e))?;
            out.push(format!("{} {}{}", rel, PREFIX, blake3::hash(&b).to_hex()));
        }
    }
    Ok(())
}

/// The body of the section named `name` (up to the next heading of the same or a higher level).
pub fn section<'a>(text: &'a str, name: &str) -> Option<String> {
    let lines: Vec<&'a str> = text.lines().collect();
    let (at, level) = lines.iter().enumerate().find_map(|(i, l)| heading(l).filter(|(_, t)| t.eq_ignore_ascii_case(name)).map(|(lv, _)| (i, lv)))?;
    let end = lines[at + 1..].iter().position(|l| heading(l).is_some_and(|(lv, _)| lv <= level)).map(|p| at + 1 + p).unwrap_or(lines.len());
    Some(lines[at + 1..end].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# Pre-registration\n\n## Hypotheses\nH1: A3b passes more.\n\n## Task set\ntasks blake3:aa\n\n## Deviations\n\n## Appendix\nstays\n";

    #[test]
    fn a_deviation_appended_under_its_heading_does_not_move_the_pin_and_any_other_edit_does() {
        let pin0 = pin(FILE);
        assert!(pin0.starts_with("blake3:") && pin0.len() == 7 + 64);
        let with_dev = FILE.replace("## Deviations\n", "## Deviations\n- 2026-11-02: one run excluded, the arm crashed.\n");
        assert_eq!(pin(&with_dev), pin0);
        assert_ne!(pin(&FILE.replace("H1: A3b passes more.", "H1: A3b passes less.")), pin0);
        assert_ne!(pin(&FILE.replace("stays", "moved")), pin0, "text after the Deviations section is pinned");
        assert_ne!(pin(&FILE.replace("## Deviations\n", "## Deviations\n\n## Extra\n")), pin0);
    }

    #[test]
    fn digest_of_a_directory_names_its_files_and_changes_with_any_byte() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("a.md"), "one").unwrap();
        std::fs::create_dir(t.path().join("sub")).unwrap();
        std::fs::write(t.path().join("sub/b.md"), "two").unwrap();
        let d0 = digest(t.path()).unwrap();
        assert_eq!(digest(t.path()).unwrap(), d0);
        std::fs::write(t.path().join("sub/b.md"), "twp").unwrap();
        assert_ne!(digest(t.path()).unwrap(), d0);
        assert_eq!(digest(&t.path().join("a.md")).unwrap(), format!("blake3:{}", blake3::hash(b"one").to_hex()));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("a.md", t.path().join("l.md")).unwrap();
            assert!(digest(t.path()).unwrap_err().contains("symlink"));
        }
    }

    #[test]
    fn sections_are_read_by_heading() {
        assert_eq!(section(FILE, "task set").as_deref(), Some("tasks blake3:aa\n"));
        assert!(section(FILE, "Rubric").is_none());
    }
}

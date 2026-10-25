//! Dependency-free document readers and writers for Delphi agents (`specs/014`).
//!
//! The crate is a leaf: it knows nothing of the engine, the path guard or provenance. A caller hands it bytes
//! it has already read through the guard and gets text back. Every entry point treats its input as hostile:
//! bounded work, bounded output, and a panic becomes an error result instead of taking the daemon down.

pub mod limits;
pub mod sniff;

pub use limits::Limits;
pub use sniff::{detect, Format};

use std::panic::{catch_unwind, AssertUnwindSafe};

/// What a call asks for. Unit paging fields are added with the readers that use them.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// Archive member or sheet name.
    pub member: Option<String>,
}

/// The unit a document is paged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Line,
    Page,
    Sheet,
    Slide,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extract {
    pub format: Format,
    pub text: String,
    pub unit: Unit,
    /// Units in the whole document, when known.
    pub total_units: Option<usize>,
    /// The output hit a limit and more exists.
    pub truncated: bool,
    /// What this read lost, one line each (`specs/014` §2.5). The agent must not believe it read more than it did.
    pub loss: Vec<String>,
    /// False when no reader exists for the format yet; `text` then says so.
    pub supported: bool,
}

/// Why nothing could be extracted. The text is shown to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocError(pub String);

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Run a reader so that a panic in it is an error. A hostile file must not be able to take the daemon down.
pub fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T, DocError>) -> Result<T, DocError> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(DocError(format!("the {} reader failed on this file (internal error); the file is not shown", what))),
    }
}

/// Readers that exist. Grows slice by slice (`specs/014` §10); anything else answers "not built yet".
pub fn is_supported(_format: Format) -> bool {
    false
}

/// Extract text from `bytes`. `name` is the file name (a hint only).
pub fn extract(bytes: &[u8], name: &str, _req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    if bytes.len() > limits.max_input {
        return Err(DocError(format!("the file is {} bytes; documents larger than {} bytes are not read", bytes.len(), limits.max_input)));
    }
    let format = detect(bytes, name);
    guarded(format.name(), || {
        if !is_supported(format) {
            return Ok(Extract {
                format,
                text: format!("{} ({} bytes): reading this format is not built yet.", format.name(), bytes.len()),
                unit: Unit::Line,
                total_units: None,
                truncated: false,
                loss: vec!["no reader for this format yet".to_string()],
                supported: false,
            });
        }
        Err(DocError(format!("no reader for {}", format.name())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unbuilt_format_is_named_and_flagged_not_hidden() {
        let e = extract(b"%PDF-1.4 ...", "a.pdf", &Request::default(), &Limits::default()).unwrap();
        assert_eq!(e.format, Format::Pdf);
        assert!(!e.supported);
        assert!(e.text.contains("PDF") && e.text.contains("not built yet"));
        assert_eq!(e.loss.len(), 1);
    }

    #[test]
    fn input_over_the_limit_is_refused_before_any_parsing() {
        let l = Limits { max_input: 8, ..Limits::default() };
        let err = extract(&[b'x'; 9], "a.bin", &Request::default(), &l).unwrap_err();
        assert!(err.0.contains("9 bytes"), "{}", err);
        assert!(extract(&[b'x'; 8], "a.bin", &Request::default(), &l).is_ok());
    }

    #[test]
    fn a_panicking_reader_becomes_an_error() {
        let r: Result<(), DocError> = guarded("test", || panic!("boom"));
        assert!(r.unwrap_err().0.contains("internal error"));
    }

    #[test]
    fn extract_never_panics_on_junk() {
        let mut x: u64 = 88172645463325252;
        for n in 0..500usize {
            let buf: Vec<u8> = (0..n % 300)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect();
            let _ = extract(&buf, "x.pdf", &Request::default(), &Limits::default());
        }
    }
}

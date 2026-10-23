//! Findings and outcomes shared by `lint` and `doctor` (`specs/006` §5: ok / warn / fail / skipped).

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Skipped,
    Warn,
    Fail,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Level::Ok => "ok",
            Level::Skipped => "skipped",
            Level::Warn => "warn",
            Level::Fail => "fail",
        };
        f.write_str(s)
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub level: Level,
    pub check: String,
    pub location: String,
    pub message: String,
}

#[derive(Default)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn push(&mut self, level: Level, check: &str, location: impl Into<String>, message: impl Into<String>) {
        self.findings.push(Finding { level, check: check.into(), location: location.into(), message: message.into() });
    }
    pub fn fail(&mut self, check: &str, location: impl Into<String>, message: impl Into<String>) {
        self.push(Level::Fail, check, location, message);
    }
    pub fn warn(&mut self, check: &str, location: impl Into<String>, message: impl Into<String>) {
        self.push(Level::Warn, check, location, message);
    }
    pub fn worst(&self) -> Level {
        self.findings.iter().map(|f| f.level).max().unwrap_or(Level::Ok)
    }
    pub fn count(&self, level: Level) -> usize {
        self.findings.iter().filter(|f| f.level == level).count()
    }
    /// Exit code: only `fail` is non-zero.
    pub fn exit_code(&self) -> i32 {
        if self.worst() == Level::Fail {
            1
        } else {
            0
        }
    }
    pub fn print(&self) {
        for f in &self.findings {
            if f.location.is_empty() {
                println!("[{}] {}: {}", f.level, f.check, f.message);
            } else {
                println!("[{}] {}: {}: {}", f.level, f.check, f.location, f.message);
            }
        }
    }
}

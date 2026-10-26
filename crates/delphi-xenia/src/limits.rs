//! Every size, count and length bound of spec 012 (§2, §4, §5, §6), as constants in one place so the
//! boundary tests can pin both sides of each. Tightening a limit later is a breaking change and relaxing is
//! safe (spec 012 S-3), so these start at the spec's numbers and, where the spec is silent, at the smallest
//! value a real record plausibly needs. Bounds the spec does not name are marked "(not in spec)".

pub use crate::sidecar::{MAX_RECORD_BYTES, MAX_SIDECAR_BYTES};

/// Body, checked after the frontmatter parses (spec 012 §6 step 1).
pub const MAX_BODY_BYTES: usize = 4 * 1024;

pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_SUMMARY_CHARS: usize = 500;
pub const MAX_TAGS: usize = 12;
pub const MAX_TAG_LEN: usize = 32;
pub const MAX_SLUG_LEN: usize = 32; // org, store
pub const MAX_POINTERS: usize = 8;
pub const MAX_EVIDENCE: usize = 4;
pub const MAX_RELATED: usize = 8;
pub const MAX_LABEL_CHARS: usize = 80;
pub const MAX_ACCESS_HINT_CHARS: usize = 120;
pub const MAX_ENDORSEMENT_NOTE_CHARS: usize = 300;
pub const MAX_TOMBSTONE_NOTE_CHARS: usize = 200;
pub const MAX_SCOPE: usize = 12; // same bound as tags: scope is a subset of the target's tags (not in spec)

// Bounds the spec leaves open (not in spec).
pub const MAX_HANDLE_CHARS: usize = 48;
pub const MAX_LOCATOR_CHARS: usize = 512;
pub const MAX_PIN_VALUE_CHARS: usize = 128;
pub const MAX_APPLIES_ITEMS: usize = 16;
pub const MAX_APPLIES_ITEM_CHARS: usize = 120;
pub const MAX_SCANNER_CHARS: usize = 32;
pub const MAX_SCANNER_VERSION_CHARS: usize = 16;
pub const MAX_REV: u64 = 1_000_000_000;
pub const MAX_POINTER_BYTES: u64 = 1 << 40;
pub const MAX_RANGE_LINE: u64 = 1_000_000;

// Shape of the frontmatter text itself (not in spec): these bound the parser's work and its recursion.
/// One line, in bytes. A 500-character summary is at most 2000 bytes of UTF-8, plus the key and quotes.
pub const MAX_LINE_BYTES: usize = 2560;
pub const MAX_FRONTMATTER_LINES: usize = 160;
/// Map nesting: top map (0) > `pointers` list (1) > pointer map (2) > `pin` map (3). Anything deeper is refused.
pub const MAX_DEPTH: usize = 4;
pub const MAX_KEYS_PER_MAP: usize = 40;
pub const MAX_LIST_ITEMS: usize = 64;
pub const MAX_SEQ_ITEMS: usize = 16;
pub const MAX_KEY_LEN: usize = 32;

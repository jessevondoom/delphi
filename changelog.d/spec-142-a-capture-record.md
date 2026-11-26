---
section: Added
---
- **A capture says what it is a copy of** (spec 142 A): each source version keeps the bytes as they came (`raw`, beside a text that is kept) and `capture.json` with both SHA-256 digests, who captured it, what made the text and each later revisit. The same bytes, or other bytes with the same text, are a revisit and not a new version. `delphi sources verify` reports a kept file that no longer matches its record. Sources captured earlier need no step: their record is written the next time the same bytes are read.

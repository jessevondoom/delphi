---
section: Added
---
- **Anchors say which text they count in, and orphans are shown** (spec 142 E): an anchor records the SHA-256 of the text its position counts in and is found by position only in that text, else by its quote, else by its context. The quote a claim was checked against (at most 300 characters) is pinned and never evicted. `delphi sources show`, the app's source view, a page's bibliography and `delphi kb health` show the anchors that no longer resolve, with their quote, the version and date they last resolved in, and why. `delphi sources private purge` now removes the text only: the quote, the record and the pinned excerpt stay.

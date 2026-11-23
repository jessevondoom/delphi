---
section: Changed
---
- **Release notes come from one file per change** (spec 036 §9): a pull request adds a fragment under `changelog.d/` instead of editing `CHANGELOG.md`, which many pull requests edited at once and conflicted in. `delphi release changelog` prints the next release's section, `--write <version>` folds the fragments into `CHANGELOG.md` and deletes them, and `delphi lint` fails a malformed fragment. The release workflow does not call it: the release pull request runs it.

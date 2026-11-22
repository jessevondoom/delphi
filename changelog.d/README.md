# Release-note fragments

One file per user-facing change, so pull requests never edit the same file (spec 036 §9). Do not edit `CHANGELOG.md` in a pull request: the release pull request folds these files into it with `delphi release changelog --write <version>` and deletes them.

A fragment is `changelog.d/<slug>.md`. The slug is lowercase letters, digits, `-`, `_` or `.`; name it after the change, for example `spec-130-s2-start-screen.md`, so two pull requests do not pick the same name.

```markdown
---
section: Added
---
- **What changed, in a few words** (spec NNN): what a user now sees or must do, and what was not verified.
```

- `section:` is one of `Added`, `Changed`, `Removed`, `Fixed`, `Security`, and is the only line of the frontmatter.
- The body is one or more Markdown list items (the first line starts with `- `), written the way `CHANGELOG.md` is written. No headings: the release's section headings are written when it is assembled.
- No relative links: the text moves from `changelog.d/` to the repository root, so a relative link cannot be right in both. Name a path in backticks, or link an absolute `https://` address.

`delphi release changelog` prints what the next release's section would be; `--check` checks the fragments; `delphi lint --repo .` fails a malformed one. The release section lists `Added`, `Changed`, `Removed`, `Fixed` and `Security` in that order, and the entries in each by file name; the release author writes the summary above them by hand.

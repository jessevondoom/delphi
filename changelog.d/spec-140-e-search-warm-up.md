---
section: Changed
---
- **The search service warms up before the first search** (spec 140 E): the daemon opens the knowledge-base reader and its embedding model at start and after each sync, outside any search, so a cold model load no longer makes searches miss the 800 ms deadline. A search that arrives during the warm-up answers by keywords at once and is counted `lexical-warming`. The service no longer loads the reranker it never used. `delphi kb health` shows how the daemon's searches were answered in the last 24 hours, and `delphi doctor` warns when more than 5% missed the deadline. Not yet measured on a real workspace.

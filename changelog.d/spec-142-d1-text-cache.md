---
section: Changed
---
- **The text cache has a size and gives up what is least used** (spec 142 D1): `[library] text_cache_max_mib` (default 1024, at least 16) bounds the text kept with no open licence; `private_text_max_mib` is read as its old name when the new key is absent. Past the cap the least recently used capture loses its raw bytes first and then its text; a search hit, a page gather, an open and a revisit count as use. An evicted source keeps its record, its excerpts and its claims.

# Machine-readable schemas

[`schemas/`](../../schemas/) holds [JSON Schema](https://json-schema.org/) (draft 2020-12) files for the formats other programs read or write. Use them to validate a file before Delphi sees it, to generate types, or to give an editor completion. TOML and YAML files are validated after parsing them to JSON.

| Schema | Validates | Authority |
| :-- | :-- | :-- |
| [`config.schema.json`](../../schemas/config.schema.json) | `config.toml` | shape and enumerations; `delphi doctor` is authoritative |
| [`bundle.schema.json`](../../schemas/bundle.schema.json) | `bundle.toml` (schema 1) | shape and the trust-key refusals; `delphi bundle validate` also checks files, secrets and signatures |
| [`source-meta.schema.json`](../../schemas/source-meta.schema.json) | the `meta.json` of a bundle's `sources/<id>/` | [`specs/023`](../../specs/023-org-bundle-standard.md) §2.1, [`specs/055`](../../specs/055-the-central-library-sources-claims-and-pages.md) §3.2.1; `delphi bundle validate` owns the licence warnings and the `private` check |
| [`event-envelope.schema.json`](../../schemas/event-envelope.schema.json) | one line of a mission event log, and the lines of `delphi run --jsonl` | [`specs/005`](../../specs/005-console-and-narrator-tui.md) §2.2 |
| [`handoff-envelope.schema.json`](../../schemas/handoff-envelope.schema.json) | the handoff between agents | [`specs/001`](../../specs/001-governance-and-manifests.md) §4 |
| [`xenia-record.schema.json`](../../schemas/xenia-record.schema.json) | the YAML frontmatter of a Xenia record (`memory`, `endorsement`, `tombstone`) | [`specs/012`](../../specs/012-xenia-record-schema.md); shape only, the verifier also checks signatures, limits and string hygiene |
| [`xenia-sidecar.schema.json`](../../schemas/xenia-sidecar.schema.json) | a record's `.sig.json` | [`specs/012`](../../specs/012-xenia-record-schema.md) §3 |
| [`agent-manifest.schema.json`](../../schemas/agent-manifest.schema.json) | the frontmatter of `agents/<role>/AGENT.md` | [`specs/001`](../../specs/001-governance-and-manifests.md) |
| [`skill-manifest.schema.json`](../../schemas/skill-manifest.schema.json) | the frontmatter of `skills/<name>/SKILL.md` | [`specs/001`](../../specs/001-governance-and-manifests.md) |
| [`memory-entry.schema.json`](../../schemas/memory-entry.schema.json) | the optional `kind`, `created`, `last_confirmed`, `valid_until` frontmatter of a staged note, lesson or rules file | [`specs/050`](../../specs/050-memory-lifecycle-types-supersession-and-trace-mining.md) ML-1; the program also checks the date order |
| [`validation-set.schema.json`](../../schemas/validation-set.schema.json) | the operator's labelled claim set and the verdict results file | [`specs/040`](../../specs/040-validator-role-and-fact-ratification.md) §15.4; see [`validation-set.md`](validation-set.md) |

Where the program is stricter than a schema can say (cross-references, hashes, signatures, dates relative to today), the program wins. A schema is a contract for what a file *looks like*, not a promise that Delphi will accept it.

## Examples

Validate a config with [`check-jsonschema`](https://github.com/python-jsonschema/check-jsonschema) (it reads TOML directly):

```sh
check-jsonschema --schemafile schemas/config.schema.json ~/.delphi/workspaces/work/config.toml
check-jsonschema --schemafile schemas/bundle.schema.json my-bundle/bundle.toml
```

Validate an event log line with any JSON Schema library: parse the line as JSON and check it against `schemas/event-envelope.schema.json`. The session logs are under `local/sessions/<id>/` in the workspace (older ones stay under `local/missions/<id>/`) and are written only by the daemon.

Each schema's `$id` is stable and its `description` names the specification and the source file that implement it.

## How they are kept honest

`cargo test -p delphi --test docs` checks that every schema is itself valid; that the example config, the golden bundle, every `AGENT.md` and every `SKILL.md` in this repository satisfy their schemas; and that hand-built records, events and handoffs pass while wrong ones (an agent event without its routing fields, a Xenia record claiming a trust tier, a bundle that grants permissions) fail. When you change a format, change its schema in the same pull request.

## Not covered

The event *payloads* (each event type's `payload` object) are not yet individually specified here, and neither are the JSON-RPC methods between the clients and the daemon; the envelope schema constrains only the common fields. The `delphi kb serve` HTTP API (`/v1/search`, `/v1/doc`, `/v1/image`, `/v1/documents`, `/v1/collections`, `/v1/health`) is described in [`specs/004`](../../specs/004-memory-and-coaching.md) and [`028`](../../specs/028-knowledge-window.md) and has no OpenAPI file yet.

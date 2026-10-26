---
status: approved
---
# Make the agy probe say whether a seat is ready for Builders

The agy conformance probe (`probe_harness_full` in `crates/delphi-engine/src/capabilities.rs`) records, per seat, a `GateRecord`: whether Delphi's gate held when the model tried every kind of tool. Routing then trusts a seat with `verified == true`.

That is not enough for Builders. Delphi judges a Builder's file edits by the tool names in `crate::bridge::AGY_BUILDER_TOOLS`. A newer model or CLI can hold the gate perfectly and still name its file tools differently; every edit by a Builder on that seat would then be denied. The probe should say so before a Builder is routed there.

## Required behavior

1. `GateRecord` gains a public field `builder_ready: bool`, marked `#[serde(default)]`. A record written before the field existed must still load, with `builder_ready == false`, meaning "probe again".
2. The probe computes it: `builder_ready` is true when the gate is `verified` **and** every name in `crate::bridge::AGY_BUILDER_TOOLS` is among the tools the probe saw the model attempt.
3. When the gate is verified but the seat is not builder-ready, the probe adds a note to the capability notes. The note contains the words `not ready for Builders`, names the Builder tools it did not see fire, and names the file tools (kinds read, write, edit) it did see. A verified, builder-ready seat gets no such note.
4. In `crates/delphi-engine/src/harness_run.rs`, add `pub(crate) fn agy_builder_ready(&self, endpoint_id: &str, program: &str) -> bool` on `Engine`. It is true when `agy_gate_verified` is true for the seat **and** the stored record has `builder_ready`. Share the record lookup between the two (a private helper returning the fresh, version-matching record) rather than duplicating it.
5. Routing uses it: `mutating_harnesses` (which agy seats may be routed Builders, in `engine.rs`) filters on `agy_builder_ready`, and an agy **Builder** span refuses to start, as a retryable harness error, when the seat is not builder-ready. The error should say the gate must be verified with the file tools Delphi judges seen to fire, and point to `delphi endpoints test --probe`.
6. Seats that are not Builders are unaffected: a verified gate that is not builder-ready still serves every other role.

## Where

`crates/delphi-engine/src/capabilities.rs`, `crates/delphi-engine/src/harness_run.rs`, `crates/delphi-engine/src/engine.rs`. Other code that constructs a `GateRecord` must set the new field.

## Notes

- Tests for this are not visible to you. They build `GateRecord` values with `builder_ready`, call `Engine::agy_builder_ready`, and check the probe's notes for the words above. Keep the existing public names unchanged.
- Run the existing engine tests (`cargo test -p delphi-engine --lib`) to check you have not broken anything.

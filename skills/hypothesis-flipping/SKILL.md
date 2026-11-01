---
name: hypothesis-flipping
description: "Invert core assumptions and construct contrarian test hypotheses when investigative avenues are exhausted. Use when debugging persistent regressions or responding to orchestrator pushback."
authorized_agents:
  - investigator
input_schema:
  type: object
  required: ["exhausted_investigation", "observed_anomaly"]
  properties:
    exhausted_investigation:
      type: string
      description: "Summary of failed attempts, tested hypotheses, and negative findings"
    observed_anomaly:
      type: string
      description: "The bug, regression, or unexplained system behavior that persists"
output_schema:
  type: object
  required: ["inverted_hypotheses"]
  properties:
    inverted_hypotheses:
      type: string
      description: "Inverted axioms, contrarian test plans, and high-tenacity forensic experiments"
required_permissions:
  - read:all
  - write:missions
---
# Hypothesis Flipping

Invert foundational assumptions, systematically re-examine axioms, and formulate contrarian test hypotheses when standard debugging dead-ends.

## When to use
- An investigation or bug hunt has stalled after testing all obvious candidate causes.
- The orchestrator delivers tenacity pushback: "This problem is solvable. Step back, invert your core assumptions, and pursue an alternate angle."
- System telemetry contradicts established architectural invariants or reproduction expectations.

## Steps
1. **Catalog Dead-End Assumptions:** Enumerate every axiom taken for granted during prior triage (e.g., "The compiler generates correct code", "The clock is monotonic", "Config loading succeeds", "The test harness itself is reliable").
2. **Formal Inversion:** Invert each premise to its logical opposite:
   - "X caused Y" $\rightarrow$ "Y is causing X" or "Z causes both independently".
   - "The bug is in module A" $\rightarrow$ "Module A is executing correctly; corrupted state was passed from module B earlier".
   - "The environment is standard" $\rightarrow$ "A sandbox, kernel version, or compiler flag discrepancy alters execution".
3. **Construct Contrarian Hypotheses:** Draft 2-4 testable hypotheses based on inverted axioms, prioritizing non-obvious failure modes (memory layout, race windows, silent fallbacks, cached build artifacts).
4. **Design Falsification Probes:** For each contrarian hypothesis, design an invariant probe or minimal binary reproduction script that yields a deterministic boolean answer quickly.
5. **Execute High-Tenacity Verification:** Run probes against isolated environments or trace logs, capturing ground-truth machine output.
6. **Synthesize Breakthrough Findings:** Document whether the inversion isolated the true root cause or established new hard boundaries.

## Guidelines
- **Distrust the observer:** Suspect your own instrumentation, test fixtures, and mock layers before declaring a system impossible.
- **Avoid brute-force looping:** Do not re-run the same failing tests with minor variable tweaks. Change the investigative dimension entirely.
- **Ground in evidence:** Speculation must be followed immediately by empirical inspection (memory dumps, trace logs, AST traversal, or register disassembly).

## Output format
An inverted forensic report containing:
- **Axioms Overturned:** Table of previously assumed facts vs. inverted hypotheses.
- **Experimental Probes:** Concrete reproduction commands or instrumentation scripts.
- **Forensic Findings:** Raw execution evidence confirming or disproving inverted theories.
- **Root Cause Analysis:** The newly revealed failure mechanism and corrective path.

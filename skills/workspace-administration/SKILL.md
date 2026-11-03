---
name: workspace-administration
description: "Diagnose workspace health, model endpoints, connectors, and project scopes, and formulate governed action proposals. Use when onboarding, configuring endpoints, or scaffolding projects."
authorized_agents:
  - administrator
input_schema:
  type: object
  required: ["subsystem"]
  properties:
    subsystem:
      type: string
      enum: ["doctor", "endpoints", "connectors", "projects", "bundles", "capabilities"]
      description: "Subsystem to inspect or administer"
    operation:
      type: string
      description: "Diagnostic inspection or target administrative action"
output_schema:
  type: object
  required: ["status"]
  properties:
    status:
      type: string
      enum: ["healthy", "needs_action", "proposed", "guided"]
    summary:
      type: string
      description: "Findings or outcome of the administrative inspection/proposal"
    action_card:
      type: object
      description: "Optional details of proposed action card or manual operator instructions"
required_permissions:
  - read:all
---
# Workspace Administration

Diagnose workspace state, verify model endpoint connectivity, inspect connector configurations, and propose governed administrative actions for operator approval (Spec 069).

## When to use
- When the operator asks to set up, connect, or diagnose model endpoints, connectors, or tools.
- When inspecting workspace health (`doctor`) or troubleshooting why an endpoint or tool is failing.
- When scaffolding or registering new project directory scopes (`delphi project init`, `delphi project index`).
- When managing org bundles and tool availability.

## Steps
1. **Inspect Before Acting (Tier 1):**
   - Call `inspect_system` with the relevant `subsystem` (`doctor`, `endpoints`, `connectors`, `projects`, `bundles`, `capabilities`).
   - Identify missing configurations, unreachable endpoints, unpinned connectors, or unindexed projects.
2. **Formulate Governed Proposals (Tier 2):**
   - When a state-altering command is required (e.g. `delphi project init`, `delphi project index --refresh`, `delphi connectors pin <name>`):
     - Check path containment: target paths must be inside registered project roots or `$DELPHI_HOME`. Never target `.git` or protected governance/auth paths (`[RED-01]`, `[RED-04]`).
     - Check batching: group multi-step actions into a single compound plan card rather than cascading multiple turns (`[ADM-01]`).
     - Invoke `propose_admin_action` with clear `title`, `summary`, exact `command`, and explicit `blast_radius`.
3. **Guide Sovereign Operations (Tier 3):**
   - For commands requiring personal credentials, cryptographic signing, or human ratification (`delphi auth login`, `delphi bundle sign`, `delphi decisions ratify`, `delphi validation latch raise`):
     - Never attempt autonomous invocation (these remain strictly refused inside agent seats).
     - Provide clear, step-by-step instructions and the exact shell command for the operator to execute in their own terminal session.
     - Call `inspect_system` afterward to verify that the condition has cleared.

## Output format
- **Diagnosis:** Plain summary of system state and root cause.
- **Action Taken / Proposed:** Action card details or guidance.
- **Verification:** Observed outcome or next operator step.

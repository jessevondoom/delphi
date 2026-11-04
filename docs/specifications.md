# Specifications

All in [`specs/`](../specs/); each file's `status` line says whether the operator has approved it. Nearly all are approved; the "state" column says how much is built.

| | Spec | State |
| :-- | :-- | :-- |
| 001 | [Governance and manifests](../specs/001-governance-and-manifests.md): `AGENT.md`, `SKILL.md`, handoff envelope, taint lattice, themes | built |
| 002 | [Adapter and routing](../specs/002-adapter-and-routing.md): dialects, model scaler, model pool | built |
| 003 | [Collaboration and Council](../specs/003-collaboration-and-council.md) | built; one real Council |
| 004 | [Memory, retrieval and coaching](../specs/004-memory-and-coaching.md) | search built; coaching gated |
| 005, 005a-g | [Console and narrator](../specs/005-console-and-narrator-tui.md), [avatars](../specs/005a-terminal-avatar-system.md), [decks and inbox](../specs/005b-deck-navigation-and-visual-interface.md), [inline console](../specs/005c-inline-console.md), [full-screen console](../specs/005d-full-screen-console.md), [redesign](../specs/005e-console-ux-review-and-redesign.md) and [build plan](../specs/005f-console-redesign-build-plan.md), [epic threads](../specs/005g-epic-threads.md) | built; real-terminal pass pending |
| 006 | [Installer, layout and updater](../specs/006-installer-and-updater.md) | built |
| 007 | [Federation and sync](../specs/007-federated-mesh-and-sync.md) | deferred |
| 008 | [Ambient watch, rewind, HITL tiers](../specs/008-ambient-watch-and-rewind.md) | tiers built; watches and rewind gated |
| 009 | [Model access and authentication](../specs/009-model-access-and-auth.md) | built; live Gemini OAuth unverified |
| 010 | [Connectors (MCP)](../specs/010-connectors-and-tool-access.md) | built; never run live |
| 011-013 | [Xenia trust model](../specs/011-xenia-trust-model.md), [record schema](../specs/012-xenia-record-schema.md), [Librarian](../specs/013-librarian-and-internal-retrieval.md) | 011-012: library code and member commands built, no live store; 013: not built |
| 014 | [Documents and data visualization](../specs/014-documents-and-dataviz.md) | built |
| 014 | [Relay and away mode](../specs/014-relay-and-away-mode.md) | built; no live mailbox trial |
| 015-019 | [Provisional decisions](../specs/015-provisional-decisions.md), [headless run](../specs/016-headless-run.md), [timers](../specs/017-timers.md), [operator hooks](../specs/018-operator-hooks.md), [skills import/export](../specs/019-skills-import-export.md) | built |
| 020, 021 | [Harness tool bridge](../specs/020-harness-tool-bridge.md); [harness CLI features](../specs/021-harness-cli-features.md) | built |
| 022-027 | [The sharing layer](../specs/022-xenia-sharing-overview.md), [bundle standard](../specs/023-org-bundle-standard.md), [provenance and precedence](../specs/024-bundle-trust-adoption-and-precedence.md), [compatibility](../specs/025-plugin-skill-and-tool-compatibility.md), [team alignment](../specs/026-team-alignment-and-strategist-integration.md), [authoring and evaluation](../specs/027-bundle-authoring-distribution-and-evaluation.md) | built |
| 028, 029 | [Knowledge window](../specs/028-knowledge-window.md), [links and inline reader](../specs/029-links-and-inline-reader.md) | first slices built |
| 030 | [Console sessions and conversation context](../specs/030-console-sessions-and-conversation-context.md) | built |
| 031 | [Layered learning and training mode](../specs/031-layered-learning-and-training-mode.md) | built; nothing run live |
| 032-034 | [Change log](../specs/032-change-log.md), [live usage and tool monitoring](../specs/033-live-usage-and-tool-monitoring.md), [narrator monitoring panes](../specs/034-narrator-monitoring-panes-spike.md) | built |
| 035 | [Failover without a driver](../specs/035-failover-without-a-driver.md) | built |
| 036 | [Signed releases and the start-time refresh](../specs/036-signed-releases-and-start-time-refresh.md) | built; inert until a release key exists |
| 037 | [Console deck change log](../specs/037-console-deck-change-log.md) | built |
| 037 | [Training experiments](../specs/037-training-experiments-quality-and-efficiency.md) | built; proven live on research cases |
| 038 | [Capabilities map](../specs/038-capabilities-map.md) | built |
| 039 | [Error log](../specs/039-error-log.md) | built |
| 041 | [Agent registry, routing and role extensions](../specs/041-agent-registry-routing-and-role-extensions.md) | approved; not built |
| 043 | [Knowledge window, round 2](../specs/043-knowledge-window-search-page-and-growing-corpus.md) | approved; partly built |
| 044 | [Decisions pane](../specs/044-decisions-pane.md) | approved; partly built |
| 045 | [Harness loop techniques](../specs/045-harness-loop-techniques.md) | approved; first slice built |
| 046, 047 | [Harness and OpenAI coverage](../specs/046-harness-and-openai-coverage.md); [Codex CLI as a third harness](../specs/047-codex-harness-and-coverage.md) | approved; first slice built |
| 063 | [Agent fleet behaviors, dynamic orchestration, and curated knowledge](../specs/063-agent-fleet-behaviors-and-dynamic-orchestration.md) | built |
| 064 | [Knowledge retrieval for agents and MCP](../specs/064-knowledge-retrieval-for-agents-and-mcp.md) | partly built |
| 065 | [Agent episodic memory and trace distillation](../specs/065-episodic-traces-and-agent-long-term-memory.md) | built |
| 066 | [MCP research endpoint and role identity](../specs/066-mcp-research-endpoint-and-role-identity.md) | built |
| 067 | [Fleet expansion: Administrator, Assistant, Advocate](../specs/067-fleet-expansion-administrator-assistant-advocate.md) | built |
| 069 | [Workspace administration and governed operator assistance](../specs/069-workspace-administration-and-operator-assistance.md) | built |
| 070 | [Scope of No Spec, No Build, and the artifact prompt log](../specs/070-spec-gate-scope-and-artifact-prompt-log.md) | built |
| 071 | [Host plugin marketplace and durable MCP enablement](../specs/071-host-plugin-marketplace-and-durable-mcp-enablement.md) | built for Claude Code, Copilot CLI and Codex; Gemini CLI and Antigravity not started |
| 072 | [Host cognitive parity and external orchestration](../specs/072-host-cognitive-parity-and-external-orchestration.md) | built |
| 073 | [Console adaptive contrast and dual-mode palette](../specs/073-console-adaptive-contrast-and-dual-mode-palette.md) | built |
| 074 | [A stable launch path, and `delphi connect`](../specs/074-stable-launch-path-and-delphi-connect.md) | built for Claude Code and Copilot CLI; Codex next |

Two numbers are used twice (014 and 037); the file names are unique and the specs cite each other by name. Numbers 040 and 042 are reserved for specs not yet written. The machine-readable counterparts of the specs that define a format are in [`schemas/`](../schemas/) (see [`docs/reference/schemas.md`](reference/schemas.md)).

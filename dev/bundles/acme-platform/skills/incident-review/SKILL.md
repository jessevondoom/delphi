---
name: incident-review
description: "Run Acme's blameless incident review: timeline, contributing factors, three actions with owners. Use after any Sev-1 or Sev-2."
license: Proprietary
metadata:
  delphi.authorized_agents: '["strategist","architect"]'
  delphi.input_schema: '{"type":"object","required":["incident_id"],"properties":{"incident_id":{"type":"string"}}}'
  delphi.output_schema: '{"type":"object","required":["postmortem_path"],"properties":{"postmortem_path":{"type":"string"}}}'
  delphi.required_permissions: '["read:all","write:missions"]'
---
# Incident review

1. Build the timeline from the page to resolution.
2. List contributing factors. Name systems and decisions, not people.
3. Agree three actions, each with an owner role and a date.
4. Write the result using `templates/postmortem.md`.

---
name: migration-plan
description: "Plan a database or service migration with a rollback and a data-loss note. Use before any schema change."
license: Proprietary
metadata:
  delphi.authorized_agents: '["architect","builder"]'
  delphi.input_schema: '{"type":"object","required":["change"],"properties":{"change":{"type":"string"}}}'
  delphi.output_schema: '{"type":"object","required":["plan_path"],"properties":{"plan_path":{"type":"string"}}}'
  delphi.required_permissions: '["read:all"]'
---
# Migration plan

1. State the change and the services that read or write the data.
2. Write the forward migration and the reverse migration.
3. Note what data the reverse loses, or say why none is possible.

# Acme Platform Engineering bundle

Adding this bundle gives your Delphi install how Acme builds and runs its platform.

**What loads when you add it**

- `knowledge/`: architecture, decisions and runbooks. Searched, never always loaded.
- `skills/`: two working skills (incident review, migration plan) for the strategist, architect and builder.
- `agents/`: a `reviewer` subagent that runs as a red-team span.
- `behavior/`: this year's goals, our perspectives, one rule, guidance for the architect and builder, and two deny rules.

It carries no hooks, no credentials and no permission grants. Remove it with `delphi bundle remove acme-platform`.

---
name: worktree-isolation
description: "Create and prune isolated git worktrees for a task so builds never touch the operator's checkout. Use before any code change."
authorized_agents:
  - builder
input_schema:
  type: object
  required: ["task"]
  properties:
    task:
      type: string
      description: "Task id the worktree serves"
    spec:
      type: string
      description: "Approved spec the task implements"
output_schema:
  type: object
  required: ["worktree"]
  properties:
    worktree:
      type: string
      description: "Path of the isolated worktree"
    branch:
      type: string
      description: "Branch created for the task"
required_permissions:
  - read:all
  - write:worktree
  - shell:scoped_scripts
---
# Worktree Isolation

Every code change happens in a worktree made for that task. The operator's checkout is not a workspace.

## When to use
- Before any edit to project code, and again when a task is resumed or retried.

## Steps
1. Confirm the task links an `approved` spec. If not, stop (No Spec, No Build).
2. Create the worktree on a fresh branch named for the task, from the project's current base branch. Use the runtime's worktree facility rather than raw `git` on the shared checkout.
3. Work only inside it. Keep ports, caches and environment files local to it so parallel tasks cannot collide.
4. Never write the shared `.git` directory or another worktree.
5. When the task merges or is abandoned, prune the worktree and delete its branch only after the merge is confirmed.
6. Do not use a bare `git stash` in a repository with other worktrees: the stack is shared.

## Done when
Edits exist only on the task branch in the task worktree and the original checkout shows no change.

## Pitfalls
- Deleting a branch before its merge is confirmed.
- Starting from a stale base: fetch first.

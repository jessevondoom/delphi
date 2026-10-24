---
id: sandbox-review
title: "Sandbox review: what ours guarantees, what the CLIs do, and what is still open"
author: red-team
timestamp: 2026-10-24T22:00:00Z
source_type: internal_generated
confidence: medium   # CLI behavior comes from secondary sources and public docs; our own results are from tests
status: active
source_mission: 2026-10-23-foundational-design
tags: [sandbox, review, step-3]
---

# Sandbox review

Prompted by the operator's doubt that the vendor CLIs sandbox properly, and the instruction to look at best practice before more sandbox work.

## 1. What the vendor CLIs actually do
- **Claude Code** has an OS sandbox: Seatbelt on macOS, bubblewrap with a socat bridge on Linux. It restricts file writes (reads are open with some paths denied) and sends network traffic through a domain-filtering proxy. Three limits matter to us: it is **opt-in**; by default it **fails open** (a missing dependency means commands run unsandboxed); and it covers the **Bash tool's subprocesses**, not the CLI process itself, whose built-in Read and Edit tools are governed by permission rules instead. The same engine is published as a separate wrapper (`sandbox-runtime`, a beta research preview) that can wrap a whole process.
- **Gemini CLI** offers sandboxing that is **off by default**: Seatbelt profiles on macOS (six built-ins; the default `permissive-open` allows broad reads and network), or Docker, Podman, gVisor (`runsc`) and LXC.
- One secondary source says only OpenAI's Codex ships with a sandbox on by default; unverified.

So the operator's instinct is largely right: the CLIs have real sandbox machinery, but it is optional, partial, or permissive by default. For harness mode the right design is the one in `specs/009` §7: **wrap the whole CLI process from outside** with a deny-default profile, so the CLI's own tools, hooks and MCP servers are inside the boundary.

## 2. Best practice, as far as the sources agree
- Isolation tiers: containers share the host kernel and are not a security boundary for hostile code; gVisor intercepts syscalls in userspace (compatibility cost); microVMs (Firecracker, Kata) give a separate kernel; Seatbelt, Landlock and bubblewrap are lightweight and local, and are accident-and-injection containment, not defense against a determined kernel exploit. Both bubblewrap and Seatbelt have CVE history.
- Limit file access to the project, restrict egress, block writes outside the workspace (config files are a persistence vector), and treat generated code as hostile.
- `sandbox-exec` is deprecated, but Apple's own components still use it, and Claude Code, Gemini CLI and Codex all build on Seatbelt. No replacement covers ad-hoc CLI sandboxing.

## 3. What our sandbox guarantees (each has a test against the real `sandbox-exec`)
- Reads and writes only inside granted paths; an unlisted path cannot be opened or even `stat`ed.
- No `.git` segment is writable at any depth, including one created later (macOS; `bwrap` can only re-bind existing ones).
- No network except one loopback proxy port; the proxy allows an exact host list, refuses names that resolve to private addresses, and checks that the TLS server name matches the CONNECT host.
- The environment is rebuilt from an allowlist; the profile is passed inline.
- Hard links and symlinks cannot be used to reach files outside the grant.
- **New in this review: no inherited file descriptors.** A descriptor the parent leaves open (the daemon passes its lock and readiness descriptors across its own `exec`) used to be readable inside the sandbox. The test failed on the first run; the launcher now closes every non-close-on-exec descriptor in the child before `exec`.

## 4. Is it "strictly limited"? Honest answer: strictly limited in what it can touch, not yet in what it can consume
Open gaps, none yet fixed:
1. **No resource limits.** A sandboxed process can fork without bound, fill the scratch directory, or use all memory and CPU. Needed: `RLIMIT_NPROC`, `RLIMIT_AS`, `RLIMIT_FSIZE`, CPU time, a disk quota for scratch space, and on Linux cgroups or `bwrap` limits.
2. **No wall-clock timeout or process-tree kill.** macOS has no equivalent of `--die-with-parent`. Needed: a new process group per sandboxed child, killed as a group on timeout, budget breach or daemon exit.
3. **Linux is untested here.** `bwrap` should add `--cap-drop ALL`, `no_new_privs` and a seccomp filter; user namespaces can be disabled by the distribution; creating a new `.git` cannot be forbidden by name there.
4. **Information leaks that remain by design:** `sysctl-read` (hardware and OS details), and the names of top-level directories in `/` (dyld needs to read it). Neither exposes file contents.
5. **Same user.** The sandbox shares the operator's UID and the host kernel, so a kernel or `sandbox-exec` bug is a full escape. This is the limit of every Seatbelt-based tool in section 1.
6. **System services are the real attack surface.** Each Mach service we allow (the keychain daemon, notification center) is code the sandboxed process can talk to. The default allows none; the keychain experiment would add a dozen.

## 5. Different approaches for harness mode
- **Skip the CLIs.** If Delphi talks to the model APIs directly (API keys, cloud accounts, or `oauth2` through a gateway) there is no CLI to contain, and the sandbox is needed only for code execution by the Builder. This removes the keychain problem entirely. Downside: needs API access the operator does not have today, except possibly Gemini (see below).
- **Run the CLI in a VM or container.** Gemini CLI documents Docker, Podman and gVisor backends. A microVM (Apple Virtualization, Firecracker or Kata on Linux) gives a separate kernel. Cost: a heavy dependency and a login that must be performed inside the guest, which also solves the keychain problem (the CLI would log in to its own file-based store in the VM).
- **Wrap the existing CLI only for non-credential work** (for example, Gemini through its file-based login first).
- **Credential broker.** The sandboxed process gets a short-lived, narrowly scoped token through a daemon-run local proxy that injects the real credential into the upstream request. This keeps the credential out of the process entirely, but breaks the "Delphi never touches another tool's credentials" rule for subscription logins, so it fits only API keys and `oauth2` that Delphi itself owns.

## 6. Recommendation
1. Keep the sandbox for the Builder's code execution (needed regardless), and fix gaps 1 to 3 before it runs untrusted generated code.
2. Pause the Claude-CLI-in-sandbox experiment. Do not widen the sandbox to the keychain stack on a guess.
3. Move the API, `oauth2` and MCP path ahead of harness mode (milestone M6 in `step-3-plan.md`), as the operator suggested, and revisit harness mode when that path works; the VM option can then be judged on facts.
4. If the operator wants a real test sooner: a free Gemini API key from Google AI Studio would exercise the Gemini path end to end (to be confirmed against current terms and limits).

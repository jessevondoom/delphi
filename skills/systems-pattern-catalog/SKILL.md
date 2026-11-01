---
name: systems-pattern-catalog
description: "Select and apply proven systems, protocol, concurrency, and edge architectures to technical implementations. Use when choosing data models, networking protocols, or runtime execution patterns."
authorized_agents:
  - builder
input_schema:
  type: object
  required: ["requirement"]
  properties:
    requirement:
      type: string
      description: "System requirement, workload characteristics, and performance/concurrency constraints"
    domain:
      type: string
      description: "Architectural domain: concurrency, networking protocol, memory layout, or edge execution"
output_schema:
  type: object
  required: ["pattern_recommendation"]
  properties:
    pattern_recommendation:
      type: string
      description: "Detailed pattern specification, trade-offs, protocol selection, and sample implementation layout"
required_permissions:
  - read:all
  - write:worktree
  - shell:scoped_scripts
---
# Systems Pattern Catalog

Apply established systems, networking, concurrency, and edge execution patterns to build reliable, high-performance implementations.

## When to use
- Designing protocol layers, choosing between gRPC, WebSockets, HTTP/3, or Unix domain sockets.
- Structuring concurrency models (CSP channels, actor model, event loops, thread pools, async/await).
- Designing memory layouts, zero-copy data pipelines, or cache-conscious data structures.
- Implementing edge compute, local-first synchronization, or low-latency IPC boundaries.

## Steps
1. **Profile Workload & Invariants:** Identify throughput, latency, concurrency, memory footprint, and fault-tolerance bounds.
2. **Survey Candidate Patterns:** Contrast candidate systems patterns (e.g., ring buffers, copy-on-write trees, Raft/Paxos state machines, multiplexed frame streams).
3. **Protocol & Boundary Selection:** Choose transport protocols based on connection lifecycle, framing overhead, serialization cost, and backpressure capabilities.
4. **Memory & Concurrency Modeling:** Determine ownership models, lock-free vs mutex synchronization, cancellation propagation, and task scheduling semantics.
5. **Verification & Failure Modes:** Identify edge cases (connection drops, head-of-line blocking, split-brain, memory leaks, buffer saturation).
6. **Implementation Blueprint:** Provide typed interfaces, state transitions, and step-by-step TDD verification plan.

## Guidelines
- **Zero unmeasured overhead:** Avoid complex abstractions where flat structs and simple streams suffice.
- **Backpressure is mandatory:** Any queue, channel, or buffer must define bounded capacity and backpressure or drop semantics.
- **Error domain isolation:** Errors in protocol parsing must never panic the host runtime or leak file descriptors.

## Output format
A systems engineering blueprint containing:
- **System Topology:** Data flow, actor/thread boundaries, and protocol layers.
- **Invariants & Guarantees:** Concurrency safety, memory semantics, and ordering properties.
- **Protocol Specification:** Framing, handshake, heartbeat, error handling, and serialization schema.
- **Verification Plan:** Unit, benchmark, and fault-injection test scenarios.

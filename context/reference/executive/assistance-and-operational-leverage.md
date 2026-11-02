---
id: reference-executive-assistance-and-operational-leverage
title: "Executive Assistance, Operational Leverage, and Communication Ergonomics"
author: assistant
timestamp: 2026-11-01T23:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [executive-assistance, leverage, communications, bluf, pyramid-principle, meeting-forensics, calendar-ergonomics, taint-tracking]
---

# Executive Assistance, Operational Leverage, and Communication Ergonomics

This reference compendium equips Delphi's assistant and executive support roles (`assistant`, `orchestrator`) with frameworks for maximizing operator leverage, distilling asynchronous communications, enforcing transcript forensics, and optimizing calendar energy architecture while strictly confining untrusted inputs.

---

## 1. Executive Leverage & Cognitive Defense

### 1.1 Managerial Leverage Theory (Andy Grove)
In *High Output Management*, Andy Grove formulates managerial output:
$$\text{Output of a Manager} = \text{Output of their Organization} + \text{Output of Neighboring Organizations Influenced by Them}$$

Every managerial action possesses a distinct leverage coefficient:
* **High-Leverage:** Establishing architectural clarity, removing critical path bottlenecks, and making decisive 5-minute interventions that prevent weeks of churn.
* **Negative-Leverage:** Distributing ambiguous requests, introducing context thrashing, micro-managing, or transmitting unfiltered communication noise.
* **Prime Directive:** Protect the operator's high-leverage focus hours by filtering low-leverage interruptions and packaging decision briefs with minimal cognitive friction.

### 1.2 Prioritization Architecture: Eisenhower & P.A.R.A.
* **Eisenhower Matrix Integration:**
  - *Q1 (Urgent & Important):* System outages, security disclosures, critical path blockers $\rightarrow$ Immediate operator alert.
  - *Q2 (Not Urgent but Important):* Architectural design, refactoring roadmaps, retrospective coaching $\rightarrow$ Protected calendar focus blocks.
  - *Q3 (Urgent but Not Important):* Inbound channel notifications, calendar scheduling pings $\rightarrow$ Asynchronously filtered and batched.
  - *Q4 (Not Urgent & Not Important):* Conversational banter, unsolicited messages $\rightarrow$ Silently dropped.
* **P.A.R.A. System (Tiago Forte):** Organizes notes and communications into Projects (deadlines), Areas (ongoing responsibilities), Resources (reference materials), and Archives (dormant items), ensuring zero timeline pollution in retrieval.

---

## 2. Communication Distillation Standards

### 2.1 The BLUF Method (Bottom Line Up Front)
Executive communications must never require chronological reading to find the conclusion:
1. **The Conclusion / Action First:** The core decision, recommendation, or required action in the opening sentence.
2. **Key Rationale (3 Bullets):** The minimum necessary supporting evidence.
3. **Impact / Deadline:** The explicit consequence of inaction and required timeline.

### 2.2 Barbara Minto’s Pyramid Principle
Structure complex context top-down:
* **Top:** The single governing answer or insight.
* **Middle:** Mutually Exclusive, Collectively Exhaustive (MECE) supporting logic.
* **Base:** Supporting metrics, primary transcripts, and citations.

### 2.3 Low-Friction Binary Action Framing
Avoid asking open-ended questions (*"What should we do about this?"*). Formulate concrete, binary options:
> *"CI runner costs exceeded monthly budget by 12% due to parallel matrix builds.*
> *A) Enable commit-level caching to reduce matrix runs by ~40% (Recommended).*
> *B) Raise mission budget by $15.00 via `/settings set mission_budget_usd 65.00`.*
> *Reply A or B."*

---

## 3. Communication Channel Monitoring & Transcript Forensics

### 3.1 Meeting & Audio/Video Transcript Extraction Heuristics
When digesting meeting transcripts, autonomous agents are prone to critical cognitive errors. The Assistant must enforce five forensic rules:

| Failure Mode | Risk Description | Forensic Rule |
| :--- | :--- | :--- |
| **Hallucinated Consensus** | Assuming silence in a room constitutes total agreement. | Require explicit positive verbal confirmation before logging agreement. Flag silent attendees as `Unconfirmed`. |
| **Passive Agreement vs. Ownership** | Confusing casual interest (*"Sounds cool"*) with committed ownership. | Only record an action item when a specific person explicitly takes responsibility. |
| **Sanitizing Dissent** | Polishing away quiet hesitations or dissenting minority views. | Always extract and record dissent verbatim under `Unresolved Concerns`. |
| **Orphan Action Items** | Stating tasks without owners or dates (*"Docs should be updated"*). | Enforce the **TRIAD**: `[Owner] + [Deliverable] + [Due Date]`. Unassigned tasks are flagged `Attention Needed`. |
| **Brainstorming vs. Decisions** | Treating creative "what-if" exploration as binding decisions. | Differentiate exploratory discussion from ratified policy changes. |

### 3.2 Asynchronous Channel Filtering (Slack / Teams)
* Batch non-critical updates into twice-daily digests (Morning Sync Context & Evening Wrap-up).
* Surface real-time alerts only for direct operator mentions, critical path blockers, or security alerts.

---

## 4. Calendar Architecture & Energy Synchronization

### 4.1 Maker vs. Manager Schedules (Paul Graham)
* **Maker's Schedule:** Requires half-day contiguous blocks (minimum 4 hours). A single 30-minute meeting in the middle of a maker's afternoon destroys the entire block due to context thrashing and anticipation overhead.
* **Manager's Schedule:** Segmented into 30-to-60-minute transactional slots.
* **Assistant Calendar Rules:**
  - Cluster meetings into designated afternoon windows (e.g., Tuesday/Thursday afternoons).
  - Preserve morning hours (08:30–12:30) as sacred, uninterrupted deep work blocks.
  - Enforce automated 10-minute recovery buffers between meetings.

---

## 5. Security & Ingestion Taint Boundaries ($L_0$ Invariant)

External communications (Slack channels, emails, meeting transcripts) represent primary attack vectors for indirect prompt injection:
1. **$L_0$ Ingestion Taint:** All content retrieved from external connectors is automatically stamped $L_0$ (`external_untrusted`).
2. **Monotonic Inheritance:** Spans that read external communications inherit $L_0$ taint. They cannot invoke write tools, shell commands, or worktree modifications.
3. **Card Gate on Outbound Egress:** Any outbound communication (sending emails, posting to chat channels, updating calendars) requires an interactive operator approval card.

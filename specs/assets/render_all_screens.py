import os
import glob
import subprocess

from pathlib import Path

OUTPUT_DIR = str(Path(__file__).resolve().parent)
BRAIN_DIR = os.environ.get("BRAIN_DIR", "/tmp")

# Delete older pngs in specs/assets (excluding reference directory)
for f in glob.glob(os.path.join(OUTPUT_DIR, "*.png")):
    os.remove(f)
print("Cleaned up old PNGs in assets folder.")

BASE_CSS = """
@font-face {
  font-family: 'Andale Mono';
  src: local('Andale Mono'), url('file:///System/Library/Fonts/Supplemental/Andale Mono.ttf');
}

* { box-sizing: border-box; }

body {
  margin: 0;
  padding: 28px 36px;
  font-family: 'Andale Mono', monospace;
  font-size: 12pt;
  line-height: 1.5;
  -webkit-font-smoothing: antialiased;
}

pre, code {
  font-family: 'Andale Mono', monospace;
  font-size: 12pt;
  margin: 0;
}

.divider {
  border: 0;
  border-top: 1px solid var(--border-color);
  margin: 14px 0;
}

.prompt-bar {
  display: flex;
  align-items: center;
  color: var(--text-color);
}

.cursor {
  display: inline-block;
  width: 9px;
  height: 16px;
  background-color: var(--cursor-color);
  vertical-align: middle;
  margin-left: 2px;
}

.footer {
  display: flex;
  justify-content: space-between;
  color: var(--muted-color);
  font-size: 11pt;
}

.dim { color: var(--muted-color); }
.violet { color: var(--violet); }
.cyan { color: var(--cyan); }
.amber { color: var(--amber); }
.crimson { color: var(--crimson); }
.green { color: var(--green); }
.blue { color: var(--blue); }
.bold { font-weight: bold; }
"""

DARK_THEME = """
:root {
  --bg-color: #1A1F29;
  --text-color: #F8FAFC;
  --muted-color: #94A3B8;
  --border-color: #334155;
  --cursor-color: #F8FAFC;
  --violet: #D8B4FE;
  --cyan: #38BDF8;
  --amber: #FBBF24;
  --crimson: #F87171;
  --green: #4ADE80;
  --blue: #60A5FA;
}
body {
  background-color: var(--bg-color);
  color: var(--text-color);
}
"""

LIGHT_THEME = """
:root {
  --bg-color: #FFFFFF;
  --text-color: #0F172A;
  --muted-color: #64748B;
  --border-color: #CBD5E1;
  --cursor-color: #0F172A;
  --violet: #7C3AED;
  --cyan: #0284C7;
  --amber: #D97706;
  --crimson: #DC2626;
  --green: #16A34A;
  --blue: #2563EB;
}
body {
  background-color: var(--bg-color);
  color: var(--text-color);
}
"""

# Helper function to generate clean badge with colored chassis & contrasting white/gold eyes
def badge_athena():
    return '<span class="violet">⟦</span><span style="color:#FFF;">•</span><span class="violet">─</span><span style="color:#FFF;">•</span><span class="violet">⟧</span>'

def badge_daedalus():
    return '<span class="cyan">⟦</span><span style="color:#FFF;">•</span><span class="cyan">_</span><span style="color:#FFF;">•</span><span class="cyan">⟧</span>'

def badge_hephaestus(mode="idle"):
    if mode == "thinking":
        return '<span class="amber">⟦</span><span style="color:#FACC15;">•• </span><span class="amber">⟧</span>'
    return '<span class="amber">⟦</span><span style="color:#FFF;">•</span><span class="amber">─</span><span style="color:#FFF;">•</span><span class="amber">⟧</span>'

def badge_ares(alert=False):
    if alert:
        return '<span class="crimson">⟦</span><span style="color:#FACC15;">x</span><span class="crimson">?</span><span style="color:#FACC15;">x</span><span class="crimson">⟧</span>'
    return '<span class="crimson">⟦</span><span style="color:#FFF;">-</span><span class="crimson">_</span><span style="color:#FFF;">-</span><span class="crimson">⟧</span>'

def badge_odysseus():
    return '<span class="cyan">⟦</span><span style="color:#FFF;">•</span><span class="cyan">_</span><span style="color:#FFF;">•</span><span class="cyan">⟧</span>'

def badge_argos():
    return '<span style="color:#FACC15;">⟦</span><span style="color:#FFF;">•••</span><span style="color:#FACC15;">⟧</span>'

# Footer generation helpers
def footer_html(deck_pos="center", mode="approvals", tokens="42.1k tokens", light=False):
    dot_active_color = "#2563EB" if light else "#38BDF8"
    
    if deck_pos == "center":
        nav_dots = f'<span class="dim">○</span> <span style="color:{dot_active_color};">●</span> <span class="dim">○</span>'
    elif deck_pos == "left":
        nav_dots = f'<span style="color:{dot_active_color};">●</span> <span class="dim">○</span> <span class="dim">○</span>'
    elif deck_pos == "right":
        nav_dots = f'<span class="dim">○</span> <span class="dim">○</span> <span style="color:{dot_active_color};">●</span>'
    else: # observer
        nav_dots = f'<span style="color:{dot_active_color};">●</span> <span class="dim">○</span> <span class="dim">○</span> <span class="dim">○</span>'

    return f"""
    <hr class="divider">
    <div class="footer">
      <div>{tokens} &middot; {mode} &middot; shortcuts: ?</div>
      <div>navigate: shift+arrows &middot; {nav_dots}</div>
    </div>
    """

# 1. Main Chat Dark (Active State)
HTML_CHAT_DARK_ACTIVE = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_athena()} <span class="violet bold">athena:</span> <span>decomposing architecture into 3 tasks</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○</span> <span style="color:#38BDF8;">●</span> <span class="dim">○</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #F8FAFC; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 480px;">
    <div style="margin-bottom: 22px;">
      <div style="margin-bottom: 6px;">
        {badge_athena()} <span class="violet bold">athena</span> <span class="dim">&middot; orchestrator &middot; 00:48:12</span>
      </div>
      <div style="padding-left: 20px;">
        Decomposed the interface architecture into 3 tasks:<br>
        <span class="dim">*</span> Task 1: 3-deck swipe navigation (Shift+Left / Shift+Right)<br>
        <span class="dim">*</span> Task 2: Paged viewport stabilization with persistent prompt<br>
        <span class="dim">*</span> Task 3: Full-window observer coliseum (/observe)
      </div>
    </div>

    <div style="margin-bottom: 22px;">
      <div style="margin-bottom: 6px;">
        {badge_daedalus()} <span class="cyan bold">daedalus</span> <span class="dim">&middot; architect &middot; 00:49:05</span>
      </div>
      <div style="padding-left: 20px;">
        Specification drafted and ratified:<br>
        <span class="blue bold">specs/005b-deck-navigation-and-visual-interface.md</span><br><br>
        <pre><span class="violet">pub enum</span> <span class="cyan">DeckPosition</span> {{
    ThinkingStream, <span class="dim">// Shift+Left</span>
    MainChat,       <span class="dim">// Center (less-style paged)</span>
    DecisionInbox,  <span class="dim">// Shift+Right</span>
}}</pre>
      </div>
    </div>
  </div>

  <hr class="divider">
  <div class="prompt-bar">
    <span class="dim" style="margin-right: 8px;">❯</span>
    <span>@hephaestus implement the deck router according to specs/005b</span>
    <span class="cursor"></span>
  </div>
  {footer_html("center", "approvals", "42.1k tokens")}
</body>
</html>
"""

# 2. Main Chat Dark (Idle State with Badge Strip)
HTML_CHAT_DARK_IDLE = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_athena()} {badge_daedalus()} {badge_hephaestus()} {badge_ares()} {badge_odysseus()} {badge_argos()}
        <span class="dim" style="margin-left: 12px; font-size: 11pt;">all agents idle</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○</span> <span style="color:#38BDF8;">●</span> <span class="dim">○</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #F8FAFC; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 480px;">
    <div style="margin-bottom: 22px;">
      <div style="margin-bottom: 6px;">
        {badge_athena()} <span class="violet bold">athena</span> <span class="dim">&middot; orchestrator &middot; 00:48:12</span>
      </div>
      <div style="padding-left: 20px;">
        All initial tasks completed. System is idling.<br>
        Type intent below to dispatch specialists or /observe to open telemetry.
      </div>
    </div>
  </div>

  <hr class="divider">
  <div class="prompt-bar">
    <span class="dim" style="margin-right: 8px;">❯</span>
    <span class="dim">describe intent or type / for commands</span>
    <span class="cursor"></span>
  </div>
  {footer_html("center", "auto", "42.1k tokens")}
</body>
</html>
"""

# 3. Main Chat Light Mode (Stacked Header & Active State)
HTML_CHAT_LIGHT_ACTIVE = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{LIGHT_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_athena()} <span class="violet bold">athena:</span> <span>decomposing architecture into 3 tasks</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○</span> <span style="color:#2563EB;">●</span> <span class="dim">○</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #0F172A; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 480px;">
    <div style="margin-bottom: 22px;">
      <div style="margin-bottom: 6px;">
        {badge_athena()} <span class="violet bold">athena</span> <span class="dim">&middot; orchestrator &middot; 00:48:12</span>
      </div>
      <div style="padding-left: 20px;">
        Decomposed the interface architecture into 3 tasks:<br>
        <span class="dim">*</span> Task 1: 3-deck swipe navigation (Shift+Left / Shift+Right)<br>
        <span class="dim">*</span> Task 2: Paged viewport stabilization with persistent prompt<br>
        <span class="dim">*</span> Task 3: Full-window observer coliseum (/observe)
      </div>
    </div>

    <div style="margin-bottom: 22px;">
      <div style="margin-bottom: 6px;">
        {badge_daedalus()} <span class="cyan bold">daedalus</span> <span class="dim">&middot; architect &middot; 00:49:05</span>
      </div>
      <div style="padding-left: 20px;">
        Specification drafted and ratified:<br>
        <span class="blue bold">specs/005b-deck-navigation-and-visual-interface.md</span><br><br>
        <pre><span class="violet">pub enum</span> <span class="cyan">DeckPosition</span> {{
    ThinkingStream, <span class="dim">// Shift+Left</span>
    MainChat,       <span class="dim">// Center (less-style paged)</span>
    DecisionInbox,  <span class="dim">// Shift+Right</span>
}}</pre>
      </div>
    </div>
  </div>

  <hr class="divider">
  <div class="prompt-bar">
    <span class="dim" style="margin-right: 8px;">❯</span>
    <span>@hephaestus implement the deck router according to specs/005b</span>
    <span class="cursor"></span>
  </div>
  {footer_html("center", "approvals", "42.1k tokens", light=True)}
</body>
</html>
"""

# 4. Left Deck: Activity & Thinking Stream
HTML_LEFT_ACTIVITY = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_hephaestus("thinking")} <span class="amber bold">hephaestus:</span> <span>streaming tool deltas</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span style="color:#38BDF8;">●</span> <span class="dim">○ ○</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #F8FAFC; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 480px; font-size: 11.5pt; line-height: 1.55;">
    <div style="margin-bottom: 18px;">
      <div style="margin-bottom: 2px;">
        <span class="dim">00:48:02.110</span> {badge_athena()} <span class="violet bold">athena</span> <span class="dim">&middot; orchestrator &middot; claude-3-7-sonnet &middot; thinking</span>
      </div>
      <div class="dim" style="padding-left: 20px;">
        Decomposing operator request into task graph.<br>
        Evaluating dependencies: deck-router depends on ratified spec-005b.<br>
        Routing spec review to Ares (red-team) before build dispatch.
      </div>
    </div>

    <div style="margin-bottom: 18px;">
      <div style="margin-bottom: 2px;">
        <span class="dim">00:48:05.420</span> {badge_daedalus()} <span class="cyan bold">daedalus</span> <span class="dim">&middot; architect &middot; gemini-2-flash &middot; tool_call</span>
      </div>
      <div style="padding-left: 20px;">
        <span class="dim">write_file:</span> specs/005b-deck-navigation-and-visual-interface.md (+368 lines)<br>
        <span class="dim">invariant_check:</span> [INV-AVATAR-05] 1-line badge length = 5 chars &middot; <span class="green">PASSED</span>
      </div>
    </div>

    <div style="margin-bottom: 18px;">
      <div style="margin-bottom: 2px;">
        <span class="dim">00:48:11.890</span> {badge_ares(alert=True)} <span class="crimson bold">ares</span> <span class="dim">&middot; red-team &middot; claude-3-7-sonnet &middot; review_gate</span>
      </div>
      <div style="padding-left: 20px;">
        <span class="crimson">gate_raised:</span> Red-tier approval required for canonical spec addition.<br>
        <span class="dim">payload_hash:</span> 3f9b8c2d &middot; timeout: 10m &middot; awaiting operator ratification
      </div>
    </div>

    <div style="margin-bottom: 18px;">
      <div style="margin-bottom: 2px;">
        <span class="dim">00:48:15.020</span> {badge_hephaestus()} <span class="amber bold">hephaestus</span> <span class="dim">&middot; builder &middot; claude-3-5-sonnet &middot; worktree delta</span>
      </div>
      <div style="padding-left: 20px; font-size: 11pt;">
        <span class="dim">.delphi/worktrees/t_002/crates/delphi/src/deck.rs</span><br>
        <span class="green">+ pub struct DeckRouter {{ active: DeckPosition }}</span><br>
        <span class="green">+ impl DeckRouter {{ pub fn handle_shift_arrow(&mut self, key: KeyCode) {{ ... }} }}</span><br>
        <span class="dim">Compiling delphi v0.5.0 (worktree t_002) &middot; 42.1k tokens</span>
      </div>
    </div>
  </div>

  <hr class="divider">
  <div class="prompt-bar dim">
    <span>Streaming live deltas (60fps coalesced) &middot; Shift+Right for chat</span>
  </div>
  {footer_html("left", "approvals", "42.1k tokens")}
</body>
</html>
"""

# 5. Right Deck: Plan Progress & Decision Command Center
HTML_RIGHT_PLAN_DECISIONS = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_ares(alert=True)} <span class="crimson bold">ares:</span> <span class="crimson bold">1 decision needed &middot; 09:42 remaining</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○ ○</span> <span style="color:#38BDF8;">●</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #F8FAFC; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 480px;">
    <div style="margin-bottom: 28px;">
      <div style="margin-bottom: 10px;">
        <span class="bold">Active Plan</span> <span class="dim">&middot; Mission: 2026-10-24-ui-overhaul &middot; Phase 2 of 4</span>
      </div>
      <div style="padding-left: 12px; line-height: 1.8;">
        <div><span class="green">[✓]</span> <span class="dim">t_001 Spec 005b drafted</span> <span class="dim">&middot; {badge_daedalus()} daedalus</span></div>
        <div><span class="crimson bold">[●]</span> <span class="bold">t_002 Deck router implementation</span> <span class="crimson">&middot; BLOCKED ON APPROVAL</span></div>
        <div><span class="dim">[○] t_003 Full-window observer panels &middot; {badge_hephaestus()} hephaestus &middot; queued</span></div>
        <div><span class="dim">[○] t_004 Test verification & review gates &middot; {badge_ares()} ares &middot; queued</span></div>
      </div>
    </div>

    <div style="border-top: 1px solid #2D3748; padding-top: 20px; margin-bottom: 24px;">
      <div style="margin-bottom: 8px;">
        <span class="crimson bold">Decision needed</span>
        <span class="dim" style="margin-left: 8px;">&middot; 09:42 remaining to keep task moving</span>
      </div>
      <div style="font-size: 13pt; margin-bottom: 6px;">
        <span class="blue bold">specs/005b-deck-navigation-and-visual-interface.md</span>
      </div>
      <div class="dim" style="margin-bottom: 18px; line-height: 1.5;">
        Tier: Red &middot; Requester: {badge_daedalus()} daedalus (architect) &middot; Task: t_001<br>
        Reason: Adding new protocol specification to canonical repository
      </div>

      <div style="line-height: 2.0; padding-left: 4px;">
        <div><span class="cyan bold">❯ 1.</span> <span class="bold">Accept recommendation</span> <span class="dim">&middot; Ratify spec and resume builder</span></div>
        <div class="dim" style="padding-left: 24px;">2. Request {badge_ares()} ares (red-team) security audit pass</div>
        <div class="dim" style="padding-left: 24px;">3. Defer decision until mission completion</div>
        <div class="dim" style="padding-left: 24px;">4. Reject and redirect task</div>
      </div>
    </div>
  </div>

  <hr class="divider">
  <div class="prompt-bar dim">
    <span>enter to select &middot; d for diff &middot; t to type custom reply &middot; Tab for list mode</span>
  </div>
  {footer_html("right", "approvals", "42.1k tokens")}
</body>
</html>
"""

# 6. Right Deck: Decision Inbox Focus Mode
HTML_DECISION_FOCUS = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_ares(alert=True)} <span class="crimson bold">ares:</span> <span class="crimson bold">decision needed &middot; 09:42 remaining</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○ ○</span> <span style="color:#38BDF8;">●</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #F8FAFC; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 480px; padding-top: 8px;">
    <div style="margin-bottom: 12px;">
      <span class="crimson bold">Approval required</span>
      <span class="dim" style="margin-left: 10px;">&middot; 1 of 3 pending &middot; Red-tier gate</span>
    </div>

    <div style="margin-bottom: 10px; font-size: 13pt;">
      <span class="blue bold">specs/005b-deck-navigation-and-visual-interface.md</span>
    </div>

    <div class="dim" style="margin-bottom: 22px; line-height: 1.6;">
      Requester: {badge_daedalus()} daedalus (architect) &middot; Task: t_001<br>
      Reason: Adding new protocol specification to canonical repository
    </div>

    <div style="line-height: 2.1; margin-bottom: 28px;">
      <div><span class="cyan bold">❯ 1.</span> <span class="bold">Accept recommendation</span> <span class="dim">&middot; Ratify spec and resume builder</span></div>
      <div class="dim" style="padding-left: 24px;">2. Request {badge_ares()} ares (red-team) security audit pass</div>
      <div class="dim" style="padding-left: 24px;">3. Defer decision until mission completion</div>
      <div class="dim" style="padding-left: 24px;">4. Reject and redirect task</div>
    </div>

    <div style="font-size: 11pt; line-height: 1.45;">
      <span class="dim">--- a/specs/005b-deck-navigation-and-visual-interface.md</span><br>
      <span class="dim">+++ b/specs/005b-deck-navigation-and-visual-interface.md</span><br>
      <span class="dim">@@ -0,0 +1,5 @@</span><br>
      <span class="green">+ # Spec 005b: Console Decks, Operator Inbox, Multi-Narrator</span><br>
      <span class="green">+ status: approved</span><br>
      <span class="green">+ approved_by: operator (2026-10-24)</span>
    </div>
  </div>

  <hr class="divider">
  <div class="prompt-bar dim">
    <span>enter to select &middot; d for diff &middot; t to type reply &middot; Tab for list mode</span>
  </div>
  {footer_html("right", "approvals", "42.1k tokens")}
</body>
</html>
"""

# 7. Right Deck: Decision Inbox List Mode
HTML_DECISION_LIST = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_ares(alert=True)} <span class="crimson bold">ares:</span> <span class="crimson bold">3 decisions pending</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○ ○</span> <span style="color:#38BDF8;">●</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #F8FAFC; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 480px;">
    <div style="margin-bottom: 20px;">
      <span class="bold">Pending decisions</span> <span class="dim">(3)</span>
    </div>

    <div style="line-height: 2.2;">
      <div>
        <span class="cyan bold">❯ 1.</span>
        <span class="crimson bold" style="margin-right: 12px;">RED</span>
        <span class="bold" style="display: inline-block; width: 340px;">Ratify spec 005b</span>
        <span class="dim" style="margin-right: 20px;">{badge_daedalus()} daedalus</span>
        <span class="dim">09:42</span>
      </div>
      <div>
        <span style="display: inline-block; width: 24px;"></span>
        <span class="amber bold" style="margin-right: 12px;">AMB</span>
        <span style="display: inline-block; width: 340px;">crates/delphi/src/deck.rs (+184)</span>
        <span class="dim" style="margin-right: 20px;">{badge_hephaestus()} hephaestus</span>
        <span class="dim">08:15</span>
      </div>
      <div>
        <span style="display: inline-block; width: 24px;"></span>
        <span class="amber bold" style="margin-right: 12px;">AMB</span>
        <span style="display: inline-block; width: 340px;">cargo test -p delphi --lib</span>
        <span class="dim" style="margin-right: 20px;">{badge_hephaestus()} hephaestus</span>
        <span class="dim">04:30</span>
      </div>
    </div>

    <div style="margin-top: 48px; padding-top: 16px; border-top: 1px dashed #2D3748;" class="dim">
      Selected: #1 &middot; Ratify spec 005b into canonical repository<br>
      Reason: Red-tier gate: Adding new protocol specification to canonical repository
    </div>
  </div>

  <hr class="divider">
  <div class="prompt-bar dim">
    <span>enter to open card &middot; y to approve &middot; n to reject &middot; a to approve all amber &middot; Tab for focus mode</span>
  </div>
  {footer_html("right", "approvals", "42.1k tokens")}
</body>
</html>
"""

# 8. Slash Command Menu
HTML_SLASH_MENU = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 24px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px;">
      <div>
        {badge_athena()} <span class="violet bold">athena:</span> <span class="dim">idle</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○</span> <span style="color:#38BDF8;">●</span> <span class="dim">○</span>
      </div>
    </div>
    <div class="dim" style="font-size: 11pt;">
      <span style="color: #F8FAFC; font-weight: bold;">delphi</span> v0.5 &middot; ~/projects/work/delphi
    </div>
  </div>

  <div style="min-height: 340px; color: #94A3B8;">
    <div>{badge_athena()} athena &middot; orchestrator &middot; 00:48:12</div>
    <div style="padding-left: 20px; margin-bottom: 16px;">
      All merge gates passed. System ready for observation.
    </div>
  </div>

  <div style="padding-left: 8px; line-height: 1.8; margin-bottom: 8px;">
    <div><span class="blue bold">❯ /observe-mascots</span>    <span class="dim">command &middot; Open full-window 3-line Mascot Roster view</span></div>
    <div><span class="blue">  /observe-council</span>    <span class="dim">command &middot; Open full-window Council Chamber coliseum</span></div>
    <div><span class="blue">  /observe-spend</span>      <span class="dim">command &middot; Open 30-minute token spend sparklines</span></div>
    <div><span class="blue">  /permissions</span>        <span class="dim">command &middot; Configure approval mode (auto vs high-approvals)</span></div>
  </div>

  <hr class="divider">
  <div class="prompt-bar">
    <span class="dim" style="margin-right: 8px;">❯</span>
    <span>/obs</span>
    <span class="cursor"></span>
  </div>
  {footer_html("center", "approvals", "42.1k tokens")}
</body>
</html>
"""

# 9. Observer: Alternate Agents Panel (3-Line Mascots View)
HTML_OBSERVER_MASCOTS = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8">
<style>
{BASE_CSS}
{DARK_THEME}
.mascot-row {{
  display: flex;
  margin-bottom: 22px;
  align-items: flex-start;
}}
.mascot-col {{
  width: 90px;
  font-size: 13pt;
  line-height: 1.15;
  font-family: 'Andale Mono', monospace;
  margin-right: 24px;
}}
.info-col {{
  flex: 1;
  line-height: 1.5;
}}
</style>
</head>
<body>
  <div style="margin-bottom: 28px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline;">
      <div>
        <span class="bold">delphi observer</span> <span class="dim">&middot; [agents & rosters &middot; mascot view]</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span style="color:#38BDF8;">●</span> <span class="dim">○ ○ ○</span>
      </div>
    </div>
    <div class="dim" style="margin-top: 4px;">
      Workspace: delphi &middot; Mission: 2026-10-24-ui-overhaul
    </div>
  </div>

  <div style="min-height: 480px;">
    <div class="mascot-row">
      <div class="mascot-col violet">
        <span style="color: #FACC15;">  ▄▄▄  </span><br>
        <span>  <span style="color:#FFF;">•</span>─<span style="color:#FFF;">•</span>  </span><br>
        <span>▀▀▀▀▀▀▀</span>
      </div>
      <div class="info-col">
        <div><span class="violet bold">athena</span> <span class="dim">&middot; orchestrator &middot; idle</span></div>
        <div class="dim">claude-3-7-sonnet (frontier) &middot; 18.4k tokens &middot; runtime: 14m 20s</div>
        <div style="margin-top: 2px;">Status: Decomposed architecture into 3 tasks; awaiting builder output.</div>
      </div>
    </div>

    <div class="mascot-row">
      <div class="mascot-col cyan">
        <span style="color: #FFFFFF;">▄▄▄▄▄▄▄</span><br>
        <span>  <span style="color:#FFF;">•</span>_<span style="color:#FFF;">•</span>  </span><br>
        <span>▀▀▀▀▀▀▀</span>
      </div>
      <div class="info-col">
        <div><span class="cyan bold">daedalus</span> <span class="dim">&middot; architect &middot; idle</span></div>
        <div class="dim">gemini-2-flash (workhorse) &middot; 12.1k tokens &middot; runtime: 8m 10s</div>
        <div style="margin-top: 2px;">Status: Drafted specs/005b; verified invariant [INV-AVATAR-05].</div>
      </div>
    </div>

    <div class="mascot-row">
      <div class="mascot-col amber">
        <span style="color: #F43F5E;">▄</span><span style="color: #FFFFFF;">▄▄▄▄▄</span><span style="color: #F43F5E;">▄</span><br>
        <span> <span style="color:#FFF;">•</span>───<span style="color:#FFF;">•</span> </span><br>
        <span>▀▀▀▀▀▀▀</span>
      </div>
      <div class="info-col">
        <div><span class="amber bold">hephaestus</span> <span class="amber bold">&middot; builder &middot; running</span></div>
        <div class="dim">claude-3-5-sonnet (workhorse) &middot; 31.8k tokens &middot; runtime: 22m 45s</div>
        <div style="margin-top: 2px;">Status: Implementing DeckRouter in .delphi/worktrees/t_002/ (+184 lines).</div>
      </div>
    </div>

    <div class="mascot-row">
      <div class="mascot-col crimson">
        <span style="color: #FACC15;">▄     ▄</span><br>
        <span>  <span style="color:#FACC15;">x</span>?<span style="color:#FACC15;">x</span>  </span><br>
        <span>▀▀▀▀▀▀▀</span>
      </div>
      <div class="info-col">
        <div><span class="crimson bold">ares</span> <span class="crimson bold">&middot; red-team &middot; waiting on you</span></div>
        <div class="dim">claude-3-7-sonnet (frontier) &middot; 8.2k tokens &middot; runtime: 6m 02s</div>
        <div style="margin-top: 2px;"><span class="crimson">Status: Raised Red-tier gate: operator ratification required for spec-005b.</span></div>
      </div>
    </div>
  </div>

  <div style="border-top: 1px solid #334155; padding-top: 14px; margin-top: 12px; display: flex; justify-content: space-between; align-items: baseline; font-size: 11pt;" class="dim">
    <div>Spend Sparkline (5m): <span class="cyan">▂▃▅▇█▅▃▂</span> &middot; Cache hit ratio: 91%</div>
    <div>76.6k total tokens across 4 active agents</div>
  </div>
  {footer_html("observer", "observer", "76.6k tokens")}
</body>
</html>
"""

# 10. Observer: Agents Table
HTML_OBSERVER_AGENTS = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 28px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline;">
      <div>
        <span class="bold">delphi observer</span> <span class="dim">&middot; [active agents & telemetry]</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○</span> <span style="color:#38BDF8;">●</span> <span class="dim">○ ○</span>
      </div>
    </div>
    <div class="dim" style="margin-top: 4px;">
      Workspace: delphi &middot; Mission: 2026-10-24-ui-overhaul
    </div>
  </div>

  <div style="min-height: 420px; line-height: 2.1;">
    <div class="dim" style="display: flex; border-bottom: 1px solid #2D3748; padding-bottom: 6px; margin-bottom: 8px;">
      <span style="width: 150px;">ROLE</span>
      <span style="width: 140px;">NAME</span>
      <span style="width: 120px;">STATUS</span>
      <span style="width: 230px;">MODEL (PROVIDER / ID)</span>
      <span style="width: 130px;">TOKENS</span>
      <span style="flex: 1;">CURRENT TASK</span>
    </div>

    <div style="display: flex;">
      <span class="violet bold" style="width: 150px;">orchestrator</span>
      <span style="width: 140px;">athena</span>
      <span class="dim" style="width: 120px;">idle</span>
      <span class="dim" style="width: 230px;">anthropic &middot; claude-3-7</span>
      <span style="width: 130px;">18.4k</span>
      <span class="dim" style="flex: 1;">t_000 (mission plan)</span>
    </div>

    <div style="display: flex;">
      <span class="cyan bold" style="width: 150px;">architect</span>
      <span style="width: 140px;">daedalus</span>
      <span class="dim" style="width: 120px;">idle</span>
      <span class="dim" style="width: 230px;">google &middot; gemini-2-flash</span>
      <span style="width: 130px;">12.1k</span>
      <span class="dim" style="flex: 1;">t_001 (spec-005b)</span>
    </div>

    <div style="display: flex;">
      <span class="amber bold" style="width: 150px;">builder</span>
      <span style="width: 140px;">hephaestus</span>
      <span class="amber" style="width: 120px;">running</span>
      <span class="dim" style="width: 230px;">anthropic &middot; claude-3-5</span>
      <span style="width: 130px;">31.8k</span>
      <span style="flex: 1;">t_002 (deck-router)</span>
    </div>

    <div style="display: flex;">
      <span class="crimson bold" style="width: 150px;">red-team</span>
      <span style="width: 140px;">ares</span>
      <span class="crimson" style="width: 120px;">waiting</span>
      <span class="dim" style="width: 230px;">anthropic &middot; claude-3-7</span>
      <span style="width: 130px;">8.2k</span>
      <span class="dim" style="flex: 1;">t_001 (spec audit)</span>
    </div>

    <div style="display: flex;" class="dim">
      <span style="width: 150px;">strategist</span>
      <span style="width: 140px;">odysseus</span>
      <span style="width: 120px;">standby</span>
      <span style="width: 230px;">google &middot; gemini-2-flash</span>
      <span style="width: 130px;">4.1k</span>
      <span style="flex: 1;">--</span>
    </div>
  </div>

  <div style="border-top: 1px solid #2D3748; padding-top: 14px; margin-top: 12px; display: flex; justify-content: space-between; align-items: baseline; font-size: 11pt;" class="dim">
    <div>Spend Sparkline (5m): <span class="cyan">▂▃▅▇█▅▃▂</span> &middot; Cache hit ratio: 91%</div>
    <div>76.6k total tokens across 4 active agents</div>
  </div>
  {footer_html("observer", "observer", "76.6k tokens")}
</body>
</html>
"""

# 11. Observer: Council Chamber Live
HTML_OBSERVER_COUNCIL = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 28px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline;">
      <div>
        <span class="bold">delphi observer</span> <span class="dim">&middot; [council chamber &middot; round 2 live]</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○ ○</span> <span style="color:#38BDF8;">●</span> <span class="dim">○</span>
      </div>
    </div>
    <div class="dim" style="margin-top: 4px;">
      Debate: OSC 8 Terminal Hyperlinks vs Raw Text Paths in Multiplexers
    </div>
  </div>

  <div style="display: flex; gap: 48px; min-height: 280px; margin-bottom: 24px;">
    <div style="flex: 1;">
      <div style="margin-bottom: 6px;">
        <span class="cyan bold">daedalus</span> <span class="dim">&middot; architect &middot; Round 2 defense</span>
      </div>
      <div style="line-height: 1.6;">
        <span class="bold">Primary Stance:</span> Implement OSC 8 hyperlinks for seamless terminal jumps.<br><br>
        <span class="dim">Evolution in Round 2:</span><br>
        In response to Ares's critique on tmux escape stripping, I agree to wrap all terminal links in an adaptive layer:
        detect terminal capabilities at startup; if inside tmux with passthrough disabled, fall back to plain text path format automatically.
      </div>
    </div>

    <div style="flex: 1;">
      <div style="margin-bottom: 6px;">
        <span class="crimson bold">ares</span> <span class="dim">&middot; red-team &middot; Round 2 cross-examination</span>
      </div>
      <div style="line-height: 1.6;">
        <span class="bold">Attack Vector:</span> Terminal emulator fragmentation.<br><br>
        <span class="dim">Evolution in Round 2:</span><br>
        Architect's fallback concession satisfies security invariant [REV-SEC-01]. As long as raw path text remains visible to the operator even if OSC 8 escape codes are dropped by an SSH link or older PTY, I will sign off on the ratification.
      </div>
    </div>
  </div>

  <div style="border-top: 1px solid #2D3748; padding-top: 16px; margin-bottom: 24px;">
    <div style="margin-bottom: 6px;">
      <span class="violet bold">athena</span> <span class="dim">&middot; orchestrator &middot; consensus synthesis</span>
    </div>
    <div style="line-height: 1.6;">
      Consensus reached. Builder will implement OSC 8 with raw path fallback. Minority report waived by Ares.<br>
      <span class="dim">Status: Ready for operator ratification &middot; Shift+Left to review in Decision Inbox</span>
    </div>
  </div>

  <div style="display: flex; justify-content: space-between; align-items: baseline; font-size: 11pt;" class="dim">
    <div>Round 1: 100% blind barrier &middot; Round 2: Completed</div>
    <div>Cache Hit: 94% &middot; Latency: 1.2s avg</div>
  </div>
  {footer_html("observer", "observer", "76.6k tokens")}
</body>
</html>
"""

# 12. Observer: Spend Sparklines Over Time
HTML_OBSERVER_SPARKLINE = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{BASE_CSS}{DARK_THEME}</style></head>
<body>
  <div style="margin-bottom: 28px;">
    <div style="display: flex; justify-content: space-between; align-items: baseline;">
      <div>
        <span class="bold">delphi observer</span> <span class="dim">&middot; [spend & token ledger]</span>
      </div>
      <div class="dim">
        navigate: shift+arrows &middot; <span class="dim">○ ○ ○</span> <span style="color:#38BDF8;">●</span>
      </div>
    </div>
    <div class="dim" style="margin-top: 4px;">
      Mission: 2026-10-24-ui-overhaul &middot; Ceiling: $5.00
    </div>
  </div>

  <div style="min-height: 380px;">
    <div style="margin-bottom: 16px;">
      <span class="bold">Token Burn Over Time (Last 30 Minutes)</span>
    </div>

    <div style="line-height: 1.6; font-size: 13pt; margin-bottom: 24px;">
      <div style="display: flex; align-items: baseline;">
        <span class="dim" style="width: 80px; font-size: 11pt;">high</span>
        <span class="cyan">      ▄█▄            </span>
      </div>
      <div style="display: flex; align-items: baseline;">
        <span class="dim" style="width: 80px; font-size: 11pt;">med</span>
        <span class="cyan">  ▄ ▄█████▄   ▄      </span>
      </div>
      <div style="display: flex; align-items: baseline;">
        <span class="dim" style="width: 80px; font-size: 11pt;">low</span>
        <span class="cyan">▄███████████▄███▄▄▄▄ </span>
      </div>
      <div style="padding-left: 80px; border-top: 1px solid #2D3748; margin-top: 4px; font-size: 10pt;" class="dim">
        -30m        -20m        -10m         now
      </div>
    </div>

    <div style="margin-top: 32px; line-height: 2.0;">
      <div style="margin-bottom: 8px;" class="bold">Token Distribution by Role</div>
      <div style="display: flex;"><span class="amber" style="width: 140px;">hephaestus (bld)</span> <span style="width: 100px;">31.8k tokens</span> <span class="dim">45%</span></div>
      <div style="display: flex;"><span class="violet" style="width: 140px;">athena (orch)</span>   <span style="width: 100px;">18.4k tokens</span> <span class="dim">30%</span></div>
      <div style="display: flex;"><span class="cyan" style="width: 140px;">daedalus (arch)</span> <span style="width: 100px;">12.1k tokens</span> <span class="dim">13%</span></div>
      <div style="display: flex;"><span class="crimson" style="width: 140px;">ares (red-team)</span> <span style="width: 100px;">8.2k tokens</span> <span class="dim">12%</span></div>
    </div>
  </div>

  <div style="border-top: 1px solid #2D3748; padding-top: 14px; margin-top: 12px; display: flex; justify-content: space-between; align-items: baseline; font-size: 11pt;" class="dim">
    <div>Prompt Cache Hit Ratio: <span class="green bold">91.4%</span></div>
    <div>Total: 76.6k tokens across all active seats</div>
  </div>
  {footer_html("observer", "observer", "76.6k tokens")}
</body>
</html>
"""

PAGES = [
    ("delphi_chat_dark.html", HTML_CHAT_DARK_ACTIVE, "delphi_main_chat_dark.png"),
    ("delphi_chat_idle.html", HTML_CHAT_DARK_IDLE, "delphi_main_chat_idle.png"),
    ("delphi_chat_light.html", HTML_CHAT_LIGHT_ACTIVE, "delphi_main_chat_light.png"),
    ("delphi_activity.html", HTML_LEFT_ACTIVITY, "delphi_activity_stream.png"),
    ("delphi_plan.html", HTML_RIGHT_PLAN_DECISIONS, "delphi_plan_and_decisions.png"),
    ("delphi_approval_focus.html", HTML_DECISION_FOCUS, "delphi_approval_focus_mode.png"),
    ("delphi_approval_list.html", HTML_DECISION_LIST, "delphi_approval_list_mode.png"),
    ("delphi_slash.html", HTML_SLASH_MENU, "delphi_slash_menu.png"),
    ("delphi_mascots.html", HTML_OBSERVER_MASCOTS, "delphi_narrator_mascots.png"),
    ("delphi_agents.html", HTML_OBSERVER_AGENTS, "delphi_narrator_agents.png"),
    ("delphi_council.html", HTML_OBSERVER_COUNCIL, "delphi_narrator_council_live.png"),
    ("delphi_sparkline.html", HTML_OBSERVER_SPARKLINE, "delphi_narrator_spend_sparkline.png"),
]

for html_name, html_content, png_name in PAGES:
    html_path = f"/tmp/{html_name}"
    png_path = os.path.join(OUTPUT_DIR, png_name)
    with open(html_path, "w") as f:
        f.write(html_content)
    
    cmd = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "--headless",
        "--disable-gpu",
        "--user-data-dir=/tmp/delphi-chrome-profile",
        f"--screenshot={png_path}",
        "--window-size=1050,700",
        html_path
    ]
    subprocess.run(cmd, check=True)
    print(f"Rendered: {png_name}")

print("All 12 screens rendered cleanly with updated footer and eye details!")

#!/usr/bin/env node

/**
 * Delphi Robot Avatar Terminal Preview (v3 - Refined Heights & Personalities)
 * 
 * - Heights: All 6 avatars are identical height (strictly bottom half-block ▄)
 * - Hephaestus: Rose edges + White center brow, wide craftsman mouth (·───·)
 * - Odysseus: Cutout notch crown (▄▄ ▄ ▄▄ in Ice Cyan)
 * - Ares: Sneaky squint eyes default (-_-), blinks with wide glance (·_·)
 * - Argos: 3-Eyes (· · ·) for search / discovery!
 * - Athena: Focused / working eyes by default (•─•)
 * - Thinking Left/Right: Fixed 1-char overflow (strictly 7 characters wide)
 */

const RESET = "\x1b[0m";
const BOLD = "\x1b[1m";
const DIM = "\x1b[2m";

const fg = (r, g, b) => `\x1b[38;2;${r};${g};${b}m`;
const bg = (r, g, b) => `\x1b[48;2;${r};${g};${b}m`;

// Color definitions
const WHITE = [255, 255, 255];
const BLACK = [15, 23, 42];

const AGENTS = {
  athena: {
    name: "Athena (Orchestrator)",
    role: "orchestrator",
    chassis: [168, 85, 247],   // Violet #A855F7
    crownFg: [250, 204, 21],   // Gold Diadem
    crownStr: `  ${fg(250, 204, 21)}▄▄▄${RESET}  `,
    defaultFace: "•─•",        // Focused / working eyes by default
    badgeFace: "•─•",
    blinkFace: "-─-",          // Custom blink: centered mouth stays centered (zero jump!)
    blinkBadge: "-─-",
    crownLabel: "Golden Diadem Crown",
    personalityNote: "Focused counselor eyes (•─•)"
  },
  hephaestus: {
    name: "Hephaestus (Builder)",
    role: "builder",
    chassis: [245, 158, 11],   // Amber #F59E0B
    // Crown: Rose red ends (▄) with White marble center (▄▄▄▄▄)
    crownStr: `${fg(244, 63, 94)}▄${fg(255, 255, 255)}▄▄▄▄▄${fg(244, 63, 94)}▄${RESET}`,
    defaultFace: "WIDE_MOUTH",  // Wide anvil/craftsman mouth (•───•)
    badgeFace: "•─•",
    blinkFace: "WIDE_BLINK",   // Custom blink: wide mouth stays wide (zero jump!)
    blinkBadge: "-─-",
    crownLabel: "Rose Corners + White Brow",
    personalityNote: "Wide craftsman mouth (•───•)"
  },
  daedalus: {
    name: "Daedalus (Architect)",
    role: "architect",
    chassis: [56, 189, 248],   // Cyan #38BDF8
    crownStr: `${fg(255, 255, 255)}▄▄▄▄▄▄▄${RESET}`,
    defaultFace: "•_•",
    badgeFace: "•_•",
    blinkFace: "-_-",
    blinkBadge: "-_-",
    crownLabel: "Solid White Marble Brow",
    personalityNote: "Clean precision drafting line (•_•)"
  },
  ares: {
    name: "Ares (Red-Team)",
    role: "red-team",
    chassis: [239, 68, 68],    // Crimson #EF4444
    crownStr: `${fg(250, 204, 21)}▄     ▄${RESET}`,
    defaultFace: "-_-",        // Sneaky squint default!
    badgeFace: "-_-",
    blinkFace: "•_•",          // Blinks alert
    blinkBadge: "•_•",
    crownLabel: "Bronze Horns / Spikes",
    personalityNote: "Sneaky squint default (-_-), blinks alert"
  },
  odysseus: {
    name: "Odysseus (Strategist)",
    role: "strategist",
    chassis: [20, 184, 166],   // Teal #14B8A6
    // Crown: Light Pink with centered cutout notch (▄▄ ▄ ▄▄)
    crownStr: `${fg(249, 168, 212)}▄▄ ▄ ▄▄${RESET}`,
    defaultFace: "•_•",
    badgeFace: "•_•",
    blinkFace: "-_-",
    blinkBadge: "-_-",
    crownLabel: "Light Pink Cutout Notch",
    personalityNote: "Light Pink cutout crown (•_•)"
  },
  argos: {
    name: "Argos (Scout)",
    role: "scout",
    chassis: [52, 211, 153],   // Emerald #34D399
    crownStr: `   ${fg(250, 204, 21)}▄${RESET}   `,
    defaultFace: "THREE_EYES_BULLET", // Larger dots for the 3 eyes (• • •)
    badgeFace: "•••",
    blinkFace: "THREE_EYES_BLINK",    // All 3 eyes close together (- - -)
    blinkBadge: "---",
    crownLabel: "Center Beacon Sensor",
    personalityNote: "Three larger all-seeing eyes (• • •)"
  }
};

// 1. Single-color 1-line badge: ⟦·_·⟧
// Single role color for chassis + white eyes
function renderBadge(agent, face = null, isDarkMode = true) {
  const c = fg(...agent.chassis);
  const eyeColor = isDarkMode ? fg(...WHITE) : fg(...BLACK);
  const f = face || agent.badgeFace;

  // Handle 3-eyes badge: ⟦···⟧, ⟦•••⟧, ⟦---⟧, ⟦●●●⟧, ⟦^^^⟧, ⟦•?•⟧, ⟦••• ⟧, ⟦ •••⟧
  if (["···", "•••", "---", "●●●", "^^^", "•?•", "••• ", " •••"].includes(f)) {
    return `${c}⟦${eyeColor}${f}${c}⟧${RESET}`;
  }

  // Handle waiting on approval: ⟦x?x⟧ (light x eyes, no spaces, exact 5-char width)
  if (f === "x ? x" || f === "x?x" || f === " ? ") {
    return `${c}⟦${eyeColor}${DIM}x${RESET}${eyeColor}${BOLD}?${RESET}${eyeColor}${DIM}x${RESET}${c}⟧${RESET}`;
  }

  const e1 = f[0];
  const mouth = f[1];
  const e2 = f[2];
  return `${c}⟦${eyeColor}${e1}${c}${mouth}${eyeColor}${e2}${c}⟧${RESET}`;
}

// 2. Render 3-Line Solid Rectangular Mascot
// Line 1: Crown (accent) - strictly 7 chars wide, strictly bottom-half block ▄
// Line 2: Face - strictly 7 chars wide, chassis BG, black features
// Line 3: Chin - strictly 7 chars wide (▀▀▀▀▀▀▀)
function renderSolidMascot(agent, faceOverride = null) {
  const cFg = fg(...agent.chassis);
  const cBg = bg(...agent.chassis);
  const eyeFg = fg(...BLACK);

  const l1 = agent.crownStr;
  const f = faceOverride || agent.defaultFace;

  let l2 = "";
  if (f === "THREE_EYES_BULLET") {
    // Argos 3-eyes (larger bullet dots): space, bullet, space, bullet, space, bullet, space = 7 chars!
    l2 = `${cBg}${eyeFg} • • • ${RESET}`;
  } else if (f === "THREE_EYES_BLINK") {
    // Argos 3-eyes blink: all 3 eyes close together: space, dash, space, dash, space, dash, space = 7 chars!
    l2 = `${cBg}${eyeFg} - - - ${RESET}`;
  } else if (f === "ARGOS_SCAN_L") {
    // Argos 3-eyes scan left: bullet, space, bullet, space, bullet, 2 spaces = 7 chars!
    l2 = `${cBg}${eyeFg}• • •  ${RESET}`;
  } else if (f === "ARGOS_SCAN_R") {
    // Argos 3-eyes scan right: 2 spaces, bullet, space, bullet, space, bullet = 7 chars!
    l2 = `${cBg}${eyeFg}  • • •${RESET}`;
  } else if (f === "ARGOS_PINPOINT") {
    // Argos pinpoint research mode: 3 tiny dots (· · ·)
    l2 = `${cBg}${eyeFg} · · · ${RESET}`;
  } else if (f === "ARGOS_SURPRISE") {
    // Argos surprise: 3 wide bold pupils (● ● ●)
    l2 = `${cBg}${eyeFg} ● ● ● ${RESET}`;
  } else if (f === "ARGOS_INQUIRY") {
    // Argos inquiry: normal x eyes with bold centered question mark ( x ? x )
    l2 = `${cBg}${eyeFg} x ${BOLD}?${RESET}${cBg}${eyeFg} x ${RESET}`;
  } else if (f === "x ? x" || f === "x?x" || f === " ? ") {
    // Waiting on approval: spaced x eyes with bold centered question mark ( x ? x )
    l2 = `${cBg}${eyeFg} x ${BOLD}?${RESET}${cBg}${eyeFg} x ${RESET}`;
  } else if (f === "ARGOS_HAPPY") {
    // Argos happy: 3 happy squints (^ ^ ^)
    l2 = `${cBg}${eyeFg} ^ ^ ^ ${RESET}`;
  } else if (f === "THREE_EYES") {
    // Argos 3-eyes (small dots): space, dot, space, dot, space, dot, space = 7 chars!
    l2 = `${cBg}${eyeFg} · · · ${RESET}`;
  } else if (f === "WIDE_MOUTH") {
    // Hephaestus wide mouth: space, bullet, 3-char dash, bullet, space = 7 chars!
    l2 = `${cBg}${eyeFg} •───• ${RESET}`;
  } else if (f === "WIDE_BLINK") {
    // Hephaestus wide mouth blink: space, dash, 3-char dash, dash, space = 7 chars! Zero mouth jump!
    l2 = `${cBg}${eyeFg} -───- ${RESET}`;
  } else {
    // Standard 3-char face centered with 2 spaces on each side = 7 chars!
    l2 = `${cBg}${eyeFg}  ${f}  ${RESET}`;
  }

  const l3 = `${cFg}▀▀▀▀▀▀▀${RESET}`;
  return [l1, l2, l3];
}

console.log(`${BOLD}================================================================${RESET}`);
console.log(`${BOLD} DELPHI ROBOT AVATARS (v3) — REFINED HEIGHTS & PERSONALITIES${RESET}`);
console.log(`${DIM} All avatars strictly uniform height (only bottom-half block ▄ used)${RESET}`);
console.log(`${BOLD}================================================================${RESET}\n`);

// PART 1: The Six Distinct Agents Side-by-Side
console.log(`${BOLD}1. THE SIX AGENTS (UNIFORM HEIGHT & TAILORED PERSONALITIES)${RESET}\n`);

const keys = Object.keys(AGENTS);
const rendered = keys.map(k => {
  const ag = AGENTS[k];
  return {
    ag,
    mascot: renderSolidMascot(ag),
    badge: renderBadge(ag)
  };
});

// Print side-by-side
let rowHeader = "";
let rowL1 = "";
let rowL2 = "";
let rowL3 = "";
let rowBadge = "";

rendered.forEach(r => {
  const nameShort = r.ag.name.split(" ")[0].padEnd(11);
  rowHeader += ` ${nameShort} `;
  rowL1     += `   ${r.mascot[0]}   `;
  rowL2     += `   ${r.mascot[1]}   `;
  rowL3     += `   ${r.mascot[2]}   `;
  rowBadge  += `     ${r.badge}     `;
});

console.log(rowHeader);
console.log(rowL1);
console.log(rowL2);
console.log(rowL3);
console.log(rowBadge);
console.log();

// Print breakdown
rendered.forEach(r => {
  console.log(`  ${r.badge}  ${BOLD}${r.ag.name.padEnd(26)}${RESET} ${DIM}Crown:${RESET} ${r.ag.crownLabel.padEnd(28)} ${DIM}Face:${RESET} ${r.ag.personalityNote}`);
});

console.log(`\n${BOLD}----------------------------------------------------------------${RESET}`);
console.log(`${BOLD}2. EXPRESSIONS & ANIMATIONS (Fixed 7-char width, zero chunk overflow)${RESET}\n`);

const states = [
  { name: "Idle (Default)",     face: "•_•", desc: "Default resting state (larger bullet eyes)" },
  { name: "Natural Blink",     face: "-_-", desc: "150ms sleep/blink transition" },
  { name: "Thinking (Left)",   face: "•• ", desc: "Gaze scans left (exact 7-char row)" },
  { name: "Thinking (Right)",  face: " ••", desc: "Gaze scans right (exact 7-char row)" },
  { name: "Working / Focused", face: "•─•", desc: "Running test / sandboxed tool" },
  { name: "Waiting Approval",  face: "x ? x", badge: "x?x", desc: "Spaced x eyes on mascot ( x ? x ), light tight x on badge (⟦x?x⟧)" },
  { name: "Surprise / Alert!", face: "●_●", desc: "Bold round pupils for unexpected tool return" },
  { name: "Alarm / Warning",   face: "●_!", desc: "Bold pupil + exclamation on error" },
  { name: "Success / Ratified",face: "^─^", desc: "Happy squint, spec/task approved" },
];

states.forEach(s => {
  const ag = AGENTS.hephaestus;
  const badge = renderBadge(ag, s.face);
  const mascot = renderSolidMascot(ag, s.face);

  console.log(`  ${BOLD}${s.name.padEnd(24)}${RESET} ${badge}   ${mascot[0]}   ${DIM}${s.desc}${RESET}`);
  console.log(`  ${"".padEnd(24)}         ${mascot[1]}`);
  console.log(`  ${"".padEnd(24)}         ${mascot[2]}\n`);
});

// Dedicated Argos 3-Eyed Zero-Mouth Expression Suite
console.log(`${BOLD}ARGOS 3-EYED ZERO-MOUTH SUITE (All-Seeing Scout Expressions)${RESET}\n`);
const argosStates = [
  { name: "Idle (3 Bullet Eyes)", face: "THREE_EYES_BULLET", badge: "•••", desc: "Default resting sensor state (• • •)" },
  { name: "Blink (All 3 Close)",   face: "THREE_EYES_BLINK",  badge: "---", desc: "All 3 eyes close together (- - -)" },
  { name: "Scan Left (All Panning)",face: "ARGOS_SCAN_L",     badge: "••• ",desc: "All 3 eyes pan left across face (• • •  )" },
  { name: "Scan Right (Panning)",   face: "ARGOS_SCAN_R",     badge: " •••",desc: "All 3 eyes pan right across face (  • • •)" },
  { name: "Pinpoint Research Mode", face: "ARGOS_PINPOINT",   badge: "···", desc: "3 tiny pinpoint precision dots (· · ·)" },
  { name: "Waiting on Approval",   face: "ARGOS_INQUIRY",    badge: "x?x", desc: "Spaced x eyes on mascot ( x ? x ), light tight x on badge (⟦x?x⟧)" },
  { name: "Surprise / Output!",    face: "ARGOS_SURPRISE",    badge: "●●●", desc: "All 3 eyes widen to bold circles (● ● ●)" },
  { name: "Ratified / Happy",      face: "ARGOS_HAPPY",       badge: "^^^", desc: "All 3 eyes smile in unison (^ ^ ^)" },
];

argosStates.forEach(s => {
  const ag = AGENTS.argos;
  const badge = renderBadge(ag, s.badge);
  const mascot = renderSolidMascot(ag, s.face);

  console.log(`  ${BOLD}${s.name.padEnd(26)}${RESET} ${badge}   ${mascot[0]}   ${DIM}${s.desc}${RESET}`);
  console.log(`  ${"".padEnd(26)}         ${mascot[1]}`);
  console.log(`  ${"".padEnd(26)}         ${mascot[2]}\n`);
});

console.log(`${BOLD}----------------------------------------------------------------${RESET}`);
console.log(`${BOLD}3. TUI INLINE CONTEXT (Clean Single Color + White Eyes)${RESET}\n`);

console.log(`  ${renderBadge(AGENTS.athena)}  ${BOLD}Athena${RESET}       · synthesising council round 2 verdicts [focused]`);
console.log(`  ${renderBadge(AGENTS.hephaestus)}  ${BOLD}Hephaestus${RESET}   · building worktree checkpoint [tests: 24/24 passing]`);
console.log(`  ${renderBadge(AGENTS.ares, "x?x")}  ${BOLD}Ares${RESET}         · ${fg(239, 68, 68)}APPROVAL REQUIRED${RESET} (Amber: run cargo install?)`);
console.log(`  ${renderBadge(AGENTS.daedalus, "^─^")}  ${BOLD}Daedalus${RESET}     · spec ratified (005-console-and-narrator-tui.md)`);
console.log(`  ${renderBadge(AGENTS.argos)}  ${BOLD}Argos${RESET}        · indexing repository AST [3 all-seeing eyes active]`);
console.log(`  ${renderBadge(AGENTS.odysseus, "●_●")}  ${BOLD}Odysseus${RESET}     · scope budget check (within 100%)`);
console.log();

// Interactive Animation Loop flag
if (process.argv.includes("--animate") || process.argv.includes("-a")) {
  console.log(`${BOLD}Starting live animation loop across all 6 agents... (Ctrl+C to stop)${RESET}\n`);
  
  const animStates = [
    { face: "DEFAULT", dur: 1800, label: "Idle / Standing by" },
    { face: "BLINK",   dur: 150,  label: "Blink transition" },
    { face: "DEFAULT", dur: 1000, label: "Idle / Standing by" },
    { face: "•• ",     dur: 400,  label: "Thinking (Scanning code left)" },
    { face: " ••",     dur: 400,  label: "Thinking (Scanning code right)" },
    { face: "•─•",     dur: 1500, label: "Working (Running tests in worktree)" },
    { face: "x ? x",   dur: 1800, label: "Waiting on Operator Approval (Amber)" },
    { face: "●_●",     dur: 1200, label: "Surprise! Output received (Bold pupils)" },
    { face: "^─^",     dur: 1600, label: "Success / Ratified!" },
  ];

  let sIdx = 0;
  let agIdx = 0;
  const agentList = Object.values(AGENTS);

  function tick() {
    const s = animStates[sIdx];
    const ag = agentList[agIdx];

    // Resolve agent-specific idle/blink behavior
    let faceToRender = s.face;
    let badgeFace = s.face;

    if (s.face === "DEFAULT") {
      faceToRender = ag.defaultFace;
      badgeFace = ag.badgeFace;
    } else if (s.face === "BLINK") {
      faceToRender = ag.blinkFace;
      badgeFace = ag.blinkBadge;
    } else if (ag.role === "scout") {
      // Argos stays 100% mouth-free in ALL animations!
      if (s.face === "•• ")      { faceToRender = "ARGOS_SCAN_L";  badgeFace = "••• "; }
      else if (s.face === " ••") { faceToRender = "ARGOS_SCAN_R";  badgeFace = " •••"; }
      else if (s.face === "•─•") { faceToRender = "ARGOS_PINPOINT";badgeFace = "···"; }
      else if (s.face === "•?•") { faceToRender = "ARGOS_INQUIRY"; badgeFace = "•?•"; }
      else if (s.face === "●_●") { faceToRender = "ARGOS_SURPRISE";badgeFace = "●●●"; }
      else if (s.face === "^─^") { faceToRender = "ARGOS_HAPPY";   badgeFace = "^^^"; }
    } else {
      if (faceToRender === "THREE_EYES_BULLET") badgeFace = "•••";
      else if (faceToRender === "THREE_EYES") badgeFace = "···";
      else if (faceToRender === "WIDE_MOUTH") badgeFace = "•─•";
    }

    const badge = renderBadge(ag, badgeFace);
    const mascot = renderSolidMascot(ag, faceToRender);

    process.stdout.write(`\x1b[2K  ${mascot[0]}   ${badge}  ${BOLD}${ag.name}${RESET}: ${s.label}\n`);
    process.stdout.write(`\x1b[2K  ${mascot[1]}\n`);
    process.stdout.write(`\x1b[2K  ${mascot[2]}\n\x1b[3A`);

    sIdx = (sIdx + 1) % animStates.length;
    if (sIdx === 0) agIdx = (agIdx + 1) % agentList.length;
    setTimeout(tick, s.dur);
  }
  tick();
} else {
  console.log(`${DIM}Tip: Run 'node preview_avatars.js --animate' to watch live animations!${RESET}\n`);
}

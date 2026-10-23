//! In-terminal agent avatars and status badges (`specs/005a`).
//!
//! Provides a dual-scale terminal avatar system:
//! - **1-Line Inline Badge** (5 display characters, e.g. `⟦•_•⟧`): For chat gutters, table rows,
//!   and dense telemetry logs. Uses the agent's single role color with high-contrast white/black eyes.
//! - **3-Line Solid Mascot** (7 display columns × 3 character rows): For prompt cards, approval gates,
//!   Council chamber seats, and welcome banners. Uses solid block background painting (`bg`) to
//!   achieve 100% font-immune, gapless geometry.

#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentRole {
    Athena,     // Orchestrator
    Hephaestus, // Builder
    Daedalus,   // Architect
    Ares,       // Red-Team
    Odysseus,   // Strategist
    Argos,      // Scout
    Operator,   // Human / Console
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentActivity {
    Idle,
    Blink,
    ThinkingLeft,
    ThinkingRight,
    Working,
    WaitingApproval,
    Surprise,
    Alarm,
    Success,
}

pub struct RoleTheme {
    pub name: &'static str,
    pub role_id: &'static str,
    pub chassis_rgb: (u8, u8, u8),
    pub crown_rgb: (u8, u8, u8),
    /// Strictly bottom-half block `▄` (7 display columns)
    pub crown_slice: &'static str,
    pub default_face: &'static str,
    pub badge_face: &'static str,
    pub blink_face: &'static str,
    pub blink_badge: &'static str,
}

pub const ROLES: [RoleTheme; 7] = [
    RoleTheme {
        name: "Athena",
        role_id: "orchestrator",
        chassis_rgb: (168, 85, 247), // Violet #A855F7
        crown_rgb: (250, 204, 21),   // Gold #FACC15
        crown_slice: "  ▄▄▄  ",
        default_face: "•─•",
        badge_face: "•─•",
        blink_face: "-─-",
        blink_badge: "-─-",
    },
    RoleTheme {
        name: "Hephaestus",
        role_id: "builder",
        chassis_rgb: (245, 158, 11), // Amber #F59E0B
        crown_rgb: (244, 63, 94),    // Rose corners #F43F5E
        crown_slice: "▄▄▄▄▄▄▄",       // Rose ends + White center in render
        default_face: "•───•",
        badge_face: "•─•",
        blink_face: "-───-",
        blink_badge: "-─-",
    },
    RoleTheme {
        name: "Daedalus",
        role_id: "architect",
        chassis_rgb: (56, 189, 248), // Cyan #38BDF8
        crown_rgb: (255, 255, 255),  // White marble #FFFFFF
        crown_slice: "▄▄▄▄▄▄▄",
        default_face: "•_•",
        badge_face: "•_•",
        blink_face: "-_-",
        blink_badge: "-_-",
    },
    RoleTheme {
        name: "Ares",
        role_id: "red-team",
        chassis_rgb: (239, 68, 68),  // Crimson #EF4444
        crown_rgb: (250, 204, 21),   // Bronze Gold #FACC15
        crown_slice: "▄     ▄",
        default_face: "-_-",         // Sneaky squint default
        badge_face: "-_-",
        blink_face: "•_•",          // Blinks alert
        blink_badge: "•_•",
    },
    RoleTheme {
        name: "Odysseus",
        role_id: "strategist",
        chassis_rgb: (20, 184, 166), // Teal #14B8A6
        crown_rgb: (249, 168, 212),  // Light Pink #F9A8D4
        crown_slice: "▄▄ ▄ ▄▄",
        default_face: "•_•",
        badge_face: "•_•",
        blink_face: "-_-",
        blink_badge: "-_-",
    },
    RoleTheme {
        name: "Argos",
        role_id: "scout",
        chassis_rgb: (52, 211, 153), // Emerald #34D399
        crown_rgb: (250, 204, 21),   // Gold beacon #FACC15
        crown_slice: "   ▄   ",
        default_face: "• • •",       // 3 all-seeing eyes, NO mouth
        badge_face: "•••",
        blink_face: "- - -",
        blink_badge: "---",
    },
    RoleTheme {
        name: "Operator",
        role_id: "operator",
        chassis_rgb: (148, 163, 184), // Slate #94A3B8
        crown_rgb: (203, 213, 225),
        crown_slice: "▄▄▄▄▄▄▄",
        default_face: "•_•",
        badge_face: "•_•",
        blink_face: "-_-",
        blink_badge: "-_-",
    },
];

pub fn get_theme(role: AgentRole) -> &'static RoleTheme {
    match role {
        AgentRole::Athena => &ROLES[0],
        AgentRole::Hephaestus => &ROLES[1],
        AgentRole::Daedalus => &ROLES[2],
        AgentRole::Ares => &ROLES[3],
        AgentRole::Odysseus => &ROLES[4],
        AgentRole::Argos => &ROLES[5],
        AgentRole::Operator => &ROLES[6],
    }
}

pub fn from_canonical_id(id: &str) -> Option<AgentRole> {
    match id {
        "orchestrator" | "athena" => Some(AgentRole::Athena),
        "builder" | "hephaestus" => Some(AgentRole::Hephaestus),
        "architect" | "daedalus" => Some(AgentRole::Daedalus),
        "red-team" | "ares" => Some(AgentRole::Ares),
        "strategist" | "odysseus" => Some(AgentRole::Odysseus),
        "scout" | "investigator" | "argos" => Some(AgentRole::Argos),
        "operator" | "console" => Some(AgentRole::Operator),
        _ => None,
    }
}

// ============================================================================
// ANSI String Rendering (Console / chat.rs Zero-Dependency Compatibility)
// ============================================================================

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

fn fg(rgb: (u8, u8, u8)) -> String {
    format!("\x1b[38;2;{};{};{}m", rgb.0, rgb.1, rgb.2)
}

fn bg(rgb: (u8, u8, u8)) -> String {
    format!("\x1b[48;2;{};{};{}m", rgb.0, rgb.1, rgb.2)
}

/// Renders a single-line badge (exact 5 display characters).
/// e.g. `⟦•_•⟧` or `⟦x?x⟧`
pub fn render_badge(role: AgentRole, activity: AgentActivity, is_dark_mode: bool) -> String {
    let t = get_theme(role);
    let c = fg(t.chassis_rgb);
    let eye_color = if is_dark_mode {
        "\x1b[38;2;255;255;255m" // White
    } else {
        "\x1b[38;2;15;23;42m" // Black
    };

    // Special cases
    if activity == AgentActivity::WaitingApproval {
        return format!("{c}⟦{eye_color}{DIM}x{RESET}{eye_color}{BOLD}?{RESET}{eye_color}{DIM}x{RESET}{c}⟧{RESET}");
    }

    if role == AgentRole::Argos {
        return match activity {
            AgentActivity::Idle => format!("{c}⟦{eye_color}•••{c}⟧{RESET}"),
            AgentActivity::Blink => format!("{c}⟦{eye_color}---{c}⟧{RESET}"),
            AgentActivity::ThinkingLeft => format!("{c}⟦{eye_color}••·{c}⟧{RESET}"),
            AgentActivity::ThinkingRight => format!("{c}⟦{eye_color}·••{c}⟧{RESET}"),
            AgentActivity::Working => format!("{c}⟦{eye_color}···{c}⟧{RESET}"),
            AgentActivity::Surprise => format!("{c}⟦{eye_color}●●●{c}⟧{RESET}"),
            AgentActivity::Success => format!("{c}⟦{eye_color}^^^{c}⟧{RESET}"),
            _ => format!("{c}⟦{eye_color}•••{c}⟧{RESET}"),
        };
    }

    let face = match activity {
        AgentActivity::Idle => t.badge_face,
        AgentActivity::Blink => t.blink_badge,
        AgentActivity::ThinkingLeft => "•• ",
        AgentActivity::ThinkingRight => " ••",
        AgentActivity::Working => "•─•",
        AgentActivity::WaitingApproval => "x?x",
        AgentActivity::Surprise => "●_●",
        AgentActivity::Alarm => "●_!",
        AgentActivity::Success => "^─^",
    };

    let mut chars = face.chars();
    let e1 = chars.next().unwrap_or('•');
    let m = chars.next().unwrap_or('_');
    let e2 = chars.next().unwrap_or('•');

    format!("{c}⟦{eye_color}{e1}{c}{m}{eye_color}{e2}{c}⟧{RESET}")
}

/// Renders a 3-line solid mascot (exact 7 display columns per row).
/// Line 1: Crown (bottom half block `▄`)
/// Line 2: Face (chassis background fill, black facial features)
/// Line 3: Chin (chassis `▀▀▀▀▀▀▀`, flush)
pub fn render_mascot(role: AgentRole, activity: AgentActivity) -> [String; 3] {
    let t = get_theme(role);
    let c_fg = fg(t.chassis_rgb);
    let c_bg = bg(t.chassis_rgb);
    let eye_fg = "\x1b[38;2;15;23;42m"; // Black features on solid body

    // Line 1: Crown
    let l1 = match role {
        AgentRole::Hephaestus => {
            let rose = fg(t.crown_rgb);
            let white = "\x1b[38;2;255;255;255m";
            format!("{rose}▄{white}▄▄▄▄▄{rose}▄{RESET}")
        }
        _ => format!("{}{}{RESET}", fg(t.crown_rgb), t.crown_slice),
    };

    // Line 2: Solid Face (strictly 7 characters)
    let l2 = match (role, activity) {
        // Waiting on Approval across all roles
        (_, AgentActivity::WaitingApproval) => {
            format!("{c_bg}{eye_fg} x {BOLD}?{RESET}{c_bg}{eye_fg} x {RESET}")
        }
        // Argos 3-eyed zero-mouth suite
        (AgentRole::Argos, AgentActivity::Idle) => format!("{c_bg}{eye_fg} • • • {RESET}"),
        (AgentRole::Argos, AgentActivity::Blink) => format!("{c_bg}{eye_fg} - - - {RESET}"),
        (AgentRole::Argos, AgentActivity::ThinkingLeft) => format!("{c_bg}{eye_fg}• • •  {RESET}"),
        (AgentRole::Argos, AgentActivity::ThinkingRight) => format!("{c_bg}{eye_fg}  • • •{RESET}"),
        (AgentRole::Argos, AgentActivity::Working) => format!("{c_bg}{eye_fg} · · · {RESET}"),
        (AgentRole::Argos, AgentActivity::Surprise) => format!("{c_bg}{eye_fg} ● ● ● {RESET}"),
        (AgentRole::Argos, AgentActivity::Success) => format!("{c_bg}{eye_fg} ^ ^ ^ {RESET}"),

        // Hephaestus wide mouth
        (AgentRole::Hephaestus, AgentActivity::Idle) => format!("{c_bg}{eye_fg} •───• {RESET}"),
        (AgentRole::Hephaestus, AgentActivity::Blink) => format!("{c_bg}{eye_fg} -───- {RESET}"),

        // Standard 2-eye agents
        (_, AgentActivity::Idle) => format!("{c_bg}{eye_fg}  {}  {RESET}", t.default_face),
        (_, AgentActivity::Blink) => format!("{c_bg}{eye_fg}  {}  {RESET}", t.blink_face),
        (_, AgentActivity::ThinkingLeft) => format!("{c_bg}{eye_fg}  ••   {RESET}"),
        (_, AgentActivity::ThinkingRight) => format!("{c_bg}{eye_fg}   ••  {RESET}"),
        (_, AgentActivity::Working) => format!("{c_bg}{eye_fg}  •─•  {RESET}"),
        (_, AgentActivity::Surprise) => format!("{c_bg}{eye_fg}  ●_●  {RESET}"),
        (_, AgentActivity::Alarm) => format!("{c_bg}{eye_fg}  ●_!  {RESET}"),
        (_, AgentActivity::Success) => format!("{c_bg}{eye_fg}  ^─^  {RESET}"),
    };

    // Line 3: Flush Chin
    let l3 = format!("{c_fg}▀▀▀▀▀▀▀{RESET}");

    [l1, l2, l3]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if c == 'm' {
                    in_escape = false;
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn badge_width_invariants() {
        let roles = [
            AgentRole::Athena,
            AgentRole::Hephaestus,
            AgentRole::Daedalus,
            AgentRole::Ares,
            AgentRole::Odysseus,
            AgentRole::Argos,
        ];
        let activities = [
            AgentActivity::Idle,
            AgentActivity::Blink,
            AgentActivity::ThinkingLeft,
            AgentActivity::ThinkingRight,
            AgentActivity::Working,
            AgentActivity::WaitingApproval,
            AgentActivity::Surprise,
            AgentActivity::Alarm,
            AgentActivity::Success,
        ];

        for r in roles {
            for a in activities {
                let badge = render_badge(r, a, true);
                let clean = strip_ansi(&badge);
                assert_eq!(
                    clean.chars().count(),
                    5,
                    "Badge for {:?} / {:?} must be 5 chars, was '{}'",
                    r,
                    a,
                    clean
                );
            }
        }
    }

    #[test]
    fn mascot_row_width_invariants() {
        let roles = [
            AgentRole::Athena,
            AgentRole::Hephaestus,
            AgentRole::Daedalus,
            AgentRole::Ares,
            AgentRole::Odysseus,
            AgentRole::Argos,
        ];
        let activities = [
            AgentActivity::Idle,
            AgentActivity::Blink,
            AgentActivity::ThinkingLeft,
            AgentActivity::ThinkingRight,
            AgentActivity::Working,
            AgentActivity::WaitingApproval,
            AgentActivity::Surprise,
            AgentActivity::Alarm,
            AgentActivity::Success,
        ];

        for r in roles {
            for a in activities {
                let lines = render_mascot(r, a);
                for (idx, line) in lines.iter().enumerate() {
                    let clean = strip_ansi(line);
                    assert_eq!(
                        clean.chars().count(),
                        7,
                        "Mascot {:?} / {:?} line {} must be 7 chars, was '{}'",
                        r,
                        a,
                        idx + 1,
                        clean
                    );
                }
            }
        }
    }

    #[test]
    fn argos_has_no_mouth() {
        for a in [
            AgentActivity::Idle,
            AgentActivity::Blink,
            AgentActivity::ThinkingLeft,
            AgentActivity::ThinkingRight,
            AgentActivity::Working,
            AgentActivity::WaitingApproval,
            AgentActivity::Surprise,
            AgentActivity::Success,
        ] {
            let lines = render_mascot(AgentRole::Argos, a);
            let clean = strip_ansi(&lines[1]);
            assert!(
                !clean.contains('_') && !clean.contains('─'),
                "Argos must not have a mouth in state {:?}, found '{}'",
                a,
                clean
            );
        }
    }
}

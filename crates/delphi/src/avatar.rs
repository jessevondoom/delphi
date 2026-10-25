//! In-terminal agent avatars and status badges (`specs/005a`).
//!
//! Roles are canonical IDs (`orchestrator`, `builder`, …); a theme pack supplies the display name shown
//! beside a sprite (D-17), so nothing here names a theme character.
//!
//! Provides a dual-scale terminal avatar system:
//! - **1-Line Inline Badge** (5 display characters, e.g. `⟦•_•⟧`): For chat gutters, table rows,
//!   and dense telemetry logs. Uses the agent's single role color with high-contrast white/black eyes.
//! - **3-Line Solid Mascot** (7 display columns × 3 character rows): For prompt cards, approval gates,
//!   Council chamber seats, and welcome banners. Uses solid block background painting (`bg`) to
//!   achieve 100% font-immune, gapless geometry.

// Used by the Narrator's agents pane and the console's approval cards (roadmap Step 5); nothing calls it yet.
#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentRole {
    Orchestrator,
    Builder,
    Architect,
    RedTeam,
    Strategist,
    Scout,
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
    /// The agent gave up and handed the decision to the operator (`x_x`).
    Escalated,
}

pub struct RoleTheme {
    pub role_id: &'static str,
    pub chassis_rgb: (u8, u8, u8),
    /// The ANSI colour (0-15) a 16-colour terminal gets for the chassis (`specs/005e` §5): the nearest bright colour, chosen by hand.
    pub chassis_ansi: u8,
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
        role_id: "orchestrator",
        chassis_rgb: (168, 85, 247), // Violet #A855F7
        chassis_ansi: 13,
        crown_rgb: (250, 204, 21),   // Gold #FACC15
        crown_slice: "  ▄▄▄  ",
        default_face: "•─•",
        badge_face: "•─•",
        blink_face: "-─-",
        blink_badge: "-─-",
    },
    RoleTheme {
        role_id: "builder",
        chassis_rgb: (245, 158, 11), // Amber #F59E0B
        chassis_ansi: 11,
        crown_rgb: (244, 63, 94),    // Rose corners #F43F5E
        crown_slice: "▄▄▄▄▄▄▄",       // Rose ends + White center in render
        default_face: "•───•",
        badge_face: "•─•",
        blink_face: "-───-",
        blink_badge: "-─-",
    },
    RoleTheme {
        role_id: "architect",
        chassis_rgb: (56, 189, 248), // Cyan #38BDF8
        chassis_ansi: 14,
        crown_rgb: (255, 255, 255),  // White marble #FFFFFF
        crown_slice: "▄▄▄▄▄▄▄",
        default_face: "•_•",
        badge_face: "•_•",
        blink_face: "-_-",
        blink_badge: "-_-",
    },
    RoleTheme {
        role_id: "red-team",
        chassis_rgb: (239, 68, 68),  // Crimson #EF4444
        chassis_ansi: 9,
        crown_rgb: (250, 204, 21),   // Bronze Gold #FACC15
        crown_slice: "▄     ▄",
        default_face: "-_-",         // Sneaky squint default
        badge_face: "-_-",
        blink_face: "•_•",          // Blinks alert
        blink_badge: "•_•",
    },
    RoleTheme {
        role_id: "strategist",
        chassis_rgb: (20, 184, 166), // Teal #14B8A6
        chassis_ansi: 10,
        crown_rgb: (249, 168, 212),  // Light Pink #F9A8D4
        crown_slice: "▄▄ ▄ ▄▄",
        default_face: "•_•",
        badge_face: "•_•",
        blink_face: "-_-",
        blink_badge: "-_-",
    },
    RoleTheme {
        role_id: "scout",
        chassis_rgb: (255, 255, 0), // Bright Yellow #FFFF00
        chassis_ansi: 3,
        crown_rgb: (65, 105, 225),  // Royal Blue #4169E1
        crown_slice: "   ▄   ",
        default_face: "• • •",       // 3 all-seeing eyes, NO mouth
        badge_face: "•••",
        blink_face: "- - -",
        blink_badge: "---",
    },
    RoleTheme {
        role_id: "operator",
        chassis_rgb: (148, 163, 184), // Slate #94A3B8
        chassis_ansi: 7,
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
        AgentRole::Orchestrator => &ROLES[0],
        AgentRole::Builder => &ROLES[1],
        AgentRole::Architect => &ROLES[2],
        AgentRole::RedTeam => &ROLES[3],
        AgentRole::Strategist => &ROLES[4],
        AgentRole::Scout => &ROLES[5],
        AgentRole::Operator => &ROLES[6],
    }
}

pub fn from_canonical_id(id: &str) -> Option<AgentRole> {
    match id {
        "orchestrator" => Some(AgentRole::Orchestrator),
        "builder" => Some(AgentRole::Builder),
        "architect" => Some(AgentRole::Architect),
        "red-team" => Some(AgentRole::RedTeam),
        "strategist" => Some(AgentRole::Strategist),
        // The investigator has no sprite of its own yet; it borrows the scout's (both are discovery roles).
        "scout" | "investigator" => Some(AgentRole::Scout),
        "operator" => Some(AgentRole::Operator),
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

/// Eye colours on a badge, by terminal background: bright on dark, deep slate on light (`skills/tui-design` §2.1).
pub const EYE_DARK: (u8, u8, u8) = (255, 255, 255);
pub const EYE_LIGHT: (u8, u8, u8) = (15, 23, 42);

/// The three cells between a badge's brackets. This is the one place a badge's faces are defined; the ANSI
/// renderer below and the full-screen console (through `ui_spec`) both read it.
pub fn badge_cells(role: AgentRole, activity: AgentActivity) -> String {
    use AgentActivity::*;
    let t = get_theme(role);
    match activity {
        WaitingApproval => return "x?x".into(),
        Escalated => return "x_x".into(),
        _ => {}
    }
    if role == AgentRole::Scout {
        // The scout has no mouth: it speaks only through its sensor eyes.
        return match activity {
            Blink => "---",
            ThinkingLeft => "…  ",
            ThinkingRight => "  …",
            Working => "···",
            Surprise => "●●●",
            Success => "^^^",
            _ => "•••",
        }
        .into();
    }
    match activity {
        Idle => t.badge_face,
        Blink => t.blink_badge,
        ThinkingLeft => "•• ",
        ThinkingRight => " ••",
        Working => "•─•",
        Surprise => "●_●",
        Alarm => "●_!",
        Success => "^─^",
        WaitingApproval | Escalated => unreachable!("returned above"),
    }
    .into()
}

/// Which of the three cells is the mouth (drawn in the role colour); `None` when every cell is an eye.
pub fn badge_mouth(role: AgentRole, activity: AgentActivity) -> Option<usize> {
    if role == AgentRole::Scout || activity == AgentActivity::WaitingApproval {
        None
    } else {
        Some(1)
    }
}

/// Renders a single-line badge (exact 5 display characters).
/// e.g. `⟦•_•⟧` or `⟦x?x⟧`
pub fn render_badge(role: AgentRole, activity: AgentActivity, is_dark_mode: bool) -> String {
    let t = get_theme(role);
    let c = fg(t.chassis_rgb);
    let eye_color = fg(if is_dark_mode { EYE_DARK } else { EYE_LIGHT });
    let cells: Vec<char> = badge_cells(role, activity).chars().collect();
    if activity == AgentActivity::WaitingApproval {
        return format!("{c}⟦{eye_color}{DIM}{}{RESET}{eye_color}{BOLD}{}{RESET}{eye_color}{DIM}{}{RESET}{c}⟧{RESET}", cells[0], cells[1], cells[2]);
    }
    let mouth = badge_mouth(role, activity);
    let body: String = cells.iter().enumerate().map(|(i, ch)| format!("{}{ch}", if Some(i) == mouth { &c } else { &eye_color })).collect();
    format!("{c}⟦{body}{c}⟧{RESET}")
}

/// Every activity by the name the full-screen console looks it up under.
pub const ACTIVITIES: [(&str, AgentActivity); 10] = [
    ("idle", AgentActivity::Idle),
    ("blink", AgentActivity::Blink),
    ("thinking_left", AgentActivity::ThinkingLeft),
    ("thinking_right", AgentActivity::ThinkingRight),
    ("working", AgentActivity::Working),
    ("waiting_approval", AgentActivity::WaitingApproval),
    ("surprise", AgentActivity::Surprise),
    ("alarm", AgentActivity::Alarm),
    ("success", AgentActivity::Success),
    ("escalated", AgentActivity::Escalated),
];

fn hex_rgb(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// Everything the full-screen console needs to draw badges and mascots, as data, so a new role or face added
/// above reaches it without touching TypeScript. `ui/test/fixtures/avatar-spec.json` is this value; a test keeps
/// the two equal (`DELPHI_WRITE_UI_SPEC=1 cargo test -p delphi avatar` rewrites it).
pub fn ui_spec() -> serde_json::Value {
    use serde_json::{json, Map, Value};
    let mut roles = Map::new();
    for t in ROLES.iter() {
        let Some(role) = from_canonical_id(t.role_id) else { continue };
        let badge: Map<String, Value> = ACTIVITIES.iter().map(|(name, a)| ((*name).to_string(), json!({"cells": badge_cells(role, *a), "mouth": badge_mouth(role, *a)}))).collect();
        let mascot: Map<String, Value> = ACTIVITIES.iter().map(|(name, a)| ((*name).to_string(), json!(mascot_row(role, *a)))).collect();
        let mut r = json!({
            "chassis": hex_rgb(t.chassis_rgb),
            "ansi": t.chassis_ansi,
            "crown": t.crown_slice,
            "crown_rgb": hex_rgb(t.crown_rgb),
            "mascot_face": t.default_face,
            "badge": badge,
            "mascot": mascot,
        });
        if role == AgentRole::RedTeam {
            // Gold eyes while it waits on the operator; a darker gold on light terminals, where #facc15 disappears.
            r["alert_eye"] = json!({"dark": "#facc15", "light": "#a16207"});
        }
        roles.insert(t.role_id.to_string(), r);
    }
    json!({"version": 1, "eye": {"dark": hex_rgb(EYE_DARK), "light": hex_rgb(EYE_LIGHT)}, "roles": roles})
}

/// The seven cells of a mascot's face row (eyes and mouth in dark ink on the role-coloured body).
pub fn mascot_row(role: AgentRole, activity: AgentActivity) -> String {
    use AgentActivity::*;
    let t = get_theme(role);
    match (role, activity) {
        (_, WaitingApproval) => " x ? x ".into(),
        (_, Escalated) => "  x_x  ".into(),
        // The scout has three eyes and no mouth.
        (AgentRole::Scout, Idle) => " • • • ".into(),
        (AgentRole::Scout, Blink) => " - - - ".into(),
        (AgentRole::Scout, ThinkingLeft) => "• • •  ".into(),
        (AgentRole::Scout, ThinkingRight) => "  • • •".into(),
        (AgentRole::Scout, Working) => " · · · ".into(),
        (AgentRole::Scout, Surprise) => " ● ● ● ".into(),
        (AgentRole::Scout, Success) => " ^ ^ ^ ".into(),
        // The builder has a wide mouth.
        (AgentRole::Builder, Idle) => " •───• ".into(),
        (AgentRole::Builder, Blink) => " -───- ".into(),
        (_, Idle) => format!("  {}  ", t.default_face),
        (_, Blink) => format!("  {}  ", t.blink_face),
        (_, ThinkingLeft) => "  ••   ".into(),
        (_, ThinkingRight) => "   ••  ".into(),
        (_, Working) => "  •─•  ".into(),
        (_, Surprise) => "  ●_●  ".into(),
        (_, Alarm) => "  ●_!  ".into(),
        (_, Success) => "  ^─^  ".into(),
    }
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
        AgentRole::Builder => {
            let rose = fg(t.crown_rgb);
            let white = "\x1b[38;2;255;255;255m";
            format!("{rose}▄{white}▄▄▄▄▄{rose}▄{RESET}")
        }
        _ => format!("{}{}{RESET}", fg(t.crown_rgb), t.crown_slice),
    };

    // Line 2: Solid Face (strictly 7 characters)
    let row = mascot_row(role, activity);
    let l2 = if activity == AgentActivity::WaitingApproval {
        format!("{c_bg}{eye_fg} x {BOLD}?{RESET}{c_bg}{eye_fg} x {RESET}")
    } else {
        format!("{c_bg}{eye_fg}{row}{RESET}")
    };

    // Line 3: Flush Chin
    let l3 = format!("{c_fg}▀▀▀▀▀▀▀{RESET}");

    [l1, l2, l3]
}

/// The README banner: the six working roles as sharp, theme-neutral vector art built from the same data as the
/// terminal sprites, so it cannot drift from them. `svg_banner_is_current` fails when `specs/assets/agents.svg` is stale;
/// run it with `DELPHI_WRITE_SVG=1` to rewrite the file.
pub fn svg_banner() -> String {
    const CELL: u32 = 10; // one terminal cell is 10 x 20 px
    const ROLES6: [AgentRole; 6] = [AgentRole::Orchestrator, AgentRole::Builder, AgentRole::Architect, AgentRole::RedTeam, AgentRole::Strategist, AgentRole::Scout];
    let hex = |c: (u8, u8, u8)| format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2);
    let (slot, pad) = (110u32, 20u32);
    let width = pad * 2 + slot * ROLES6.len() as u32 - (slot - 7 * CELL);
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} 170\" width=\"{width}\" height=\"170\" role=\"img\" aria-label=\"The six working Delphi agents: orchestrator, builder, architect, red-team, strategist and scout\">\n");
    out.push_str(&format!("<rect width=\"{width}\" height=\"170\" rx=\"14\" fill=\"#0d1117\"/>\n<g font-family=\"ui-monospace,SFMono-Regular,Menlo,Consolas,monospace\" text-anchor=\"middle\">\n"));
    for (i, role) in ROLES6.iter().enumerate() {
        let t = get_theme(*role);
        let x0 = pad + i as u32 * slot;
        out.push_str(&format!("<text x=\"{}\" y=\"30\" font-size=\"13\" fill=\"#c9d1d9\">{}</text>\n", x0 + 35, t.role_id));
        // Crown: the lower half of the first cell row, one block per `▄` (the builder's ends are rose, its middle white).
        for (col, _) in t.crown_slice.chars().enumerate().filter(|(_, c)| *c == '▄') {
            let colour = if *role == AgentRole::Builder && (1..=5).contains(&col) { (255, 255, 255) } else { t.crown_rgb };
            out.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{CELL}\" height=\"{}\" fill=\"{}\"/>\n", x0 + col as u32 * CELL, 50 + 10, 10, hex(colour)));
        }
        // Face: a full-height row in the role colour with dark features, then the flush chin (upper half of the next row).
        out.push_str(&format!("<rect x=\"{x0}\" y=\"70\" width=\"70\" height=\"30\" fill=\"{}\"/>\n", hex(t.chassis_rgb)));
        let face = if *role == AgentRole::Builder { "•───•" } else { t.default_face };
        let start = (7 - face.chars().count()) / 2;
        for (k, ch) in face.chars().enumerate() {
            if ch != ' ' {
                out.push_str(&format!("<text x=\"{}\" y=\"85\" font-size=\"15\" fill=\"#0f172a\">{}</text>\n", x0 + (start + k) as u32 * CELL + CELL / 2, ch));
            }
        }
        // The inline badge, as it appears in tables and the Narrator header.
        let badge = format!("⟦{}⟧", t.badge_face);
        for (k, ch) in badge.chars().enumerate() {
            let fill = if matches!(ch, '⟦' | '⟧') { hex(t.chassis_rgb) } else { "#ffffff".into() };
            out.push_str(&format!("<text x=\"{}\" y=\"138\" font-size=\"15\" fill=\"{}\">{}</text>\n", x0 + 10 + k as u32 * CELL + CELL / 2, fill, ch));
        }
    }
    out.push_str("</g>\n</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_banner_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/assets/agents.svg");
        let svg = svg_banner();
        if std::env::var("DELPHI_WRITE_SVG").is_ok() {
            std::fs::write(&path, &svg).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), svg, "specs/assets/agents.svg is stale: run `DELPHI_WRITE_SVG=1 cargo test -p delphi svg_banner`");
        assert!(svg.contains("red-team") && svg.matches("<rect").count() > 20 && !svg.contains("Athena"));
    }

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
            AgentRole::Orchestrator,
            AgentRole::Builder,
            AgentRole::Architect,
            AgentRole::RedTeam,
            AgentRole::Strategist,
            AgentRole::Scout,
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
            AgentRole::Orchestrator,
            AgentRole::Builder,
            AgentRole::Architect,
            AgentRole::RedTeam,
            AgentRole::Strategist,
            AgentRole::Scout,
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
    fn scout_has_no_mouth() {
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
            let lines = render_mascot(AgentRole::Scout, a);
            let clean = strip_ansi(&lines[1]);
            assert!(
                !clean.contains('_') && !clean.contains('─'),
                "Scout must not have a mouth in state {:?}, found '{}'",
                a,
                clean
            );
        }
    }

    /// The console draws from this data, so it must be the data the library has. Regenerate after adding a role or a face:
    /// `DELPHI_WRITE_UI_SPEC=1 cargo test -p delphi ui_spec`, then read the diff.
    #[test]
    fn ui_spec_fixture_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/test/fixtures/avatar-spec.json");
        let want = serde_json::to_string_pretty(&ui_spec()).unwrap() + "\n";
        if std::env::var("DELPHI_WRITE_UI_SPEC").is_ok() {
            std::fs::write(&path, &want).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), want, "ui/test/fixtures/avatar-spec.json is stale: run `DELPHI_WRITE_UI_SPEC=1 cargo test -p delphi ui_spec`");
    }

    /// `specs/005e` §5 names the 16-colour fallback of each role; the console reads it from `ui_spec`.
    #[test]
    fn every_role_has_a_sixteen_colour_fallback_and_the_table_of_005e_holds() {
        let want = [("orchestrator", 13), ("architect", 14), ("builder", 11), ("red-team", 9), ("strategist", 10), ("scout", 3)];
        for (role, ansi) in want {
            let t = ROLES.iter().find(|t| t.role_id == role).unwrap();
            assert_eq!(t.chassis_ansi, ansi, "{}", role);
            assert_eq!(ui_spec()["roles"][role]["ansi"], ansi, "{}", role);
        }
        assert!(ROLES.iter().all(|t| t.chassis_ansi < 16));
    }

    #[test]
    fn every_badge_state_is_five_columns_and_the_eyes_are_dark_on_light_terminals() {
        let spec = ui_spec();
        for t in ROLES.iter() {
            let role = from_canonical_id(t.role_id).unwrap();
            for (name, a) in ACTIVITIES.iter() {
                let cells = badge_cells(role, *a);
                assert_eq!(cells.chars().count(), 3, "{} {}", t.role_id, name);
                assert_eq!(spec["roles"][t.role_id]["badge"][*name]["cells"], cells.as_str());
                assert_eq!(mascot_row(role, *a).chars().count(), 7, "mascot {} {}", t.role_id, name);
                for (dark, eye) in [(true, "\x1b[38;2;255;255;255m"), (false, "\x1b[38;2;15;23;42m")] {
                    let out = render_badge(role, *a, dark);
                    assert!(out.contains(eye), "{} {} dark={}: eyes use {:?}", t.role_id, name, dark, eye);
                    assert!(!out.contains(if dark { "\x1b[38;2;15;23;42m" } else { "\x1b[38;2;255;255;255m" }), "{} {} dark={}: the other mode's eye colour leaked in", t.role_id, name, dark);
                    // Eyes are the cells that are not the mouth, whatever character they are: the red-team's `-_-` has `-` eyes.
                    if let Some(m) = badge_mouth(role, *a) {
                        let chassis = fg(t.chassis_rgb);
                        let mouth_ch = cells.chars().nth(m).unwrap();
                        assert!(out.contains(&format!("{chassis}{mouth_ch}")), "{} {} mouth is chassis-coloured", t.role_id, name);
                    }
                }
            }
        }
    }
}

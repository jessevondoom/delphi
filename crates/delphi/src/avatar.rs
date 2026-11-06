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
    // The seven roles added by spec 087: each has its own badge, blink and colours.
    Administrator,
    Assistant,
    Advocate,
    Messenger,
    Validator,
    Writer,
    Investigator,
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
    pub chassis_rgb_light: (u8, u8, u8),
    /// The ANSI colour (0-15) a 16-colour terminal gets for the chassis (`specs/005e` §5): the nearest bright colour, chosen by hand.
    pub chassis_ansi: u8,
    pub chassis_ansi_light: u8,
    pub crown_rgb: (u8, u8, u8),
    /// Strictly bottom-half block `▄` (7 display columns)
    pub crown_slice: &'static str,
    pub default_face: &'static str,
    pub badge_face: &'static str,
    pub blink_face: &'static str,
    pub blink_badge: &'static str,
}

pub const ROLES: [RoleTheme; 14] = [
    RoleTheme {
        role_id: "orchestrator",
        chassis_rgb: (168, 85, 247), // Violet #A855F7
        chassis_rgb_light: (126, 34, 206), // Deep Purple #7E22CE
        chassis_ansi: 13,
        chassis_ansi_light: 5,
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
        chassis_rgb_light: (180, 83, 9), // Deep Amber #B45309
        chassis_ansi: 11,
        chassis_ansi_light: 3,
        crown_rgb: (244, 63, 94),    // Rose corners #F43F5E
        crown_slice: "▄▄▄▄▄▄▄",       // Rose ends + White center in render
        // Spec 087: the anvil mouth. The mascot's wide form follows the badge (OD-1).
        default_face: "•===•",
        badge_face: "•=•",
        blink_face: ">===<",
        blink_badge: ">=<",
    },
    RoleTheme {
        role_id: "architect",
        chassis_rgb: (56, 189, 248), // Cyan #38BDF8
        chassis_rgb_light: (3, 105, 161), // Deep Blue #0369A1
        chassis_ansi: 14,
        chassis_ansi_light: 4,
        crown_rgb: (255, 255, 255),  // White marble #FFFFFF
        crown_slice: "▄▄▄▄▄▄▄",
        default_face: "•o•",
        badge_face: "•o•",
        blink_face: "-o-",
        blink_badge: "-o-",
    },
    RoleTheme {
        role_id: "red-team",
        chassis_rgb: (239, 68, 68),  // Crimson #EF4444
        chassis_rgb_light: (185, 28, 28), // Dark Red #B91C1C
        chassis_ansi: 9,
        chassis_ansi_light: 1,
        crown_rgb: (250, 204, 21),   // Bronze Gold #FACC15
        crown_slice: "▄     ▄",
        default_face: "¬_¬",         // Spec 087: a glare; the old squint `-_-` is now its blink
        badge_face: "¬_¬",
        blink_face: "-_-",
        blink_badge: "-_-",
    },
    RoleTheme {
        role_id: "strategist",
        chassis_rgb: (20, 184, 166), // Teal #14B8A6
        chassis_rgb_light: (15, 118, 110), // Deep Teal #0F766E
        chassis_ansi: 10,
        chassis_ansi_light: 6,
        crown_rgb: (249, 168, 212),  // Light Pink #F9A8D4
        crown_slice: "▄▄ ▄ ▄▄",
        default_face: "•u•",         // Spec 087: a wry half-smile
        badge_face: "•u•",
        blink_face: "-u-",
        blink_badge: "-u-",
    },
    RoleTheme {
        role_id: "scout",
        chassis_rgb: (255, 255, 0), // Bright Yellow #FFFF00
        chassis_rgb_light: (133, 77, 14), // Olive Gold #854D0E
        chassis_ansi: 3,
        chassis_ansi_light: 3,
        crown_rgb: (65, 105, 225),  // Royal Blue #4169E1
        crown_slice: "   ▄   ",
        default_face: "• • •",       // 3 all-seeing eyes, NO mouth
        badge_face: "•••",
        blink_face: "- - -",
        blink_badge: "---",
    },
    // The seven roles below are spec 087's. Dark and light chassis were chosen by measurement: every pair of core
    // roles stays at least `MIN_DELTA_E_DARK` / `MIN_DELTA_E_LIGHT` apart (CIELAB, tested below) and every light
    // chassis reaches 4.5:1 on white and on solarized light. The 16-colour fallbacks are distinct (ui/test/theme.test.ts
    // requires it) but only approximate in hue; at 16 colours the face and the name carry the identity.
    RoleTheme {
        role_id: "administrator",
        chassis_rgb: (104, 121, 143), // Slate #68798F (the old #64748B was 2.99:1 on #282A36, under the 3.0 gate)
        chassis_rgb_light: (91, 107, 130), // #5B6B82
        chassis_ansi: 8,
        chassis_ansi_light: 8,
        crown_rgb: (226, 232, 240),
        crown_slice: "▄ ▄ ▄ ▄",
        default_face: "•.•",
        badge_face: "•.•",
        blink_face: "-.-",
        blink_badge: "-.-",
    },
    RoleTheme {
        role_id: "assistant",
        chassis_rgb: (59, 130, 246), // Blue #3B82F6
        chassis_rgb_light: (29, 78, 216), // #1D4ED8
        chassis_ansi: 12,
        chassis_ansi_light: 4,
        crown_rgb: (147, 197, 253),
        crown_slice: "  ▄ ▄  ",
        default_face: "•‿•",
        badge_face: "•‿•",
        blink_face: "-‿-",
        blink_badge: "-‿-",
    },
    RoleTheme {
        role_id: "advocate",
        chassis_rgb: (244, 63, 94), // Rose #F43F5E
        chassis_rgb_light: (190, 18, 60), // #BE123C
        chassis_ansi: 1,
        chassis_ansi_light: 1,
        crown_rgb: (253, 224, 71),
        crown_slice: "▄  ▄  ▄",
        default_face: "•^•",
        badge_face: "•^•",
        blink_face: "*^*",
        blink_badge: "*^*",
    },
    RoleTheme {
        role_id: "messenger",
        chassis_rgb: (132, 204, 22), // Lime #84CC16
        chassis_rgb_light: (77, 124, 15), // #4D7C0F
        chassis_ansi: 4,
        chassis_ansi_light: 4,
        crown_rgb: (255, 255, 255),
        crown_slice: "▄▄   ▄▄",
        default_face: "•w•",
        badge_face: "•w•",
        blink_face: "·w·",
        blink_badge: "·w·",
    },
    RoleTheme {
        role_id: "validator",
        chassis_rgb: (34, 197, 94), // Green #22C55E
        chassis_rgb_light: (21, 128, 61), // #15803D
        chassis_ansi: 5,
        chassis_ansi_light: 5,
        crown_rgb: (250, 204, 21),
        crown_slice: " ▄▄▄▄▄ ",
        default_face: "¬_•",
        badge_face: "¬_•",
        blink_face: "-_-",
        blink_badge: "-_-",
    },
    // The writer and the investigator are twins (spec 087 §3.1): one face, told apart by colour and by mirrored winks.
    RoleTheme {
        role_id: "writer",
        chassis_rgb: (253, 230, 138), // Gold, pale #FDE68A
        chassis_rgb_light: (161, 98, 7), // #A16207
        chassis_ansi: 6,
        chassis_ansi_light: 6,
        crown_rgb: (255, 255, 255),
        crown_slice: "▄ ▄▄▄ ▄",
        default_face: "•_•",
        badge_face: "•_•",
        blink_face: "•_-",
        blink_badge: "•_-",
    },
    RoleTheme {
        role_id: "investigator",
        chassis_rgb: (241, 245, 249), // Silver #F1F5F9
        chassis_rgb_light: (87, 83, 78), // #57534E
        chassis_ansi: 15,
        chassis_ansi_light: 8,
        crown_rgb: (148, 163, 184),
        crown_slice: "▄▄ ▄ ▄▄",
        default_face: "•_•",
        badge_face: "•_•",
        blink_face: "-_•",
        blink_badge: "-_•",
    },
    RoleTheme {
        role_id: "operator",
        chassis_rgb: (148, 163, 184), // Slate #94A3B8
        chassis_rgb_light: (71, 85, 105), // Deep Slate #475569
        chassis_ansi: 7,
        chassis_ansi_light: 8,
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
        AgentRole::Administrator => &ROLES[6],
        AgentRole::Assistant => &ROLES[7],
        AgentRole::Advocate => &ROLES[8],
        AgentRole::Messenger => &ROLES[9],
        AgentRole::Validator => &ROLES[10],
        AgentRole::Writer => &ROLES[11],
        AgentRole::Investigator => &ROLES[12],
        AgentRole::Operator => &ROLES[13],
    }
}

pub fn from_canonical_id(id: &str) -> Option<AgentRole> {
    match id {
        "orchestrator" => Some(AgentRole::Orchestrator),
        "builder" => Some(AgentRole::Builder),
        "architect" => Some(AgentRole::Architect),
        "red-team" => Some(AgentRole::RedTeam),
        "strategist" => Some(AgentRole::Strategist),
        "scout" => Some(AgentRole::Scout),
        "administrator" => Some(AgentRole::Administrator),
        "assistant" => Some(AgentRole::Assistant),
        "advocate" => Some(AgentRole::Advocate),
        "messenger" => Some(AgentRole::Messenger),
        "validator" => Some(AgentRole::Validator),
        "writer" => Some(AgentRole::Writer),
        // Spec 087: the investigator no longer borrows the scout's look.
        "investigator" => Some(AgentRole::Investigator),
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
    let c = fg(if is_dark_mode { t.chassis_rgb } else { t.chassis_rgb_light });
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
            "chassis_light": hex_rgb(t.chassis_rgb_light),
            "ansi": t.chassis_ansi,
            "ansi_light": t.chassis_ansi_light,
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
        (AgentRole::Builder, Idle) => format!(" {} ", t.default_face),
        (AgentRole::Builder, Blink) => format!(" {} ", t.blink_face),
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
/// Line 3: Chin (seven spaces on a darker shade of the chassis: painted, not a `▀` glyph, which leaves a gap above it)
pub fn render_mascot(role: AgentRole, activity: AgentActivity) -> [String; 3] {
    let t = get_theme(role);
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
    let shade = |v: u8| (f32::from(v) * 0.6).round() as u8;
    let l3 = format!("{}       {RESET}", bg((shade(t.chassis_rgb.0), shade(t.chassis_rgb.1), shade(t.chassis_rgb.2))));

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
        let face = t.default_face;
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
    #[test]
    fn the_daemons_role_look_matches_the_avatar_library() {
        for t in ROLES.iter() {
            let (badge, hex, _) = delphi_core::research::role_look(t.role_id);
            let (r, g, b) = t.chassis_rgb;
            assert_eq!(hex, format!("#{r:02x}{g:02x}{b:02x}"), "{}", t.role_id);
            let role = from_canonical_id(t.role_id).unwrap();
            assert_eq!(badge, format!("⟦{}⟧", badge_cells(role, AgentActivity::Idle)), "{}", t.role_id);
        }
        // Spec 087: the investigator has a look of its own, and so has every other core role.
        assert_ne!(delphi_core::research::role_look("investigator"), delphi_core::research::role_look("scout"));
        assert_ne!(delphi_core::research::role_look("writer").1, "#94a3b8", "the writer must not fall through to the default grey");
        assert_ne!(delphi_core::research::role_look("validator").1, "#94a3b8");
        assert_ne!(delphi_core::research::role_look("messenger").1, "#94a3b8");
    }

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
        let roles: Vec<AgentRole> = ROLES.iter().filter_map(|t| from_canonical_id(t.role_id)).collect();
        assert_eq!(roles.len(), ROLES.len(), "every ROLES entry has a canonical id");
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
        let roles: Vec<AgentRole> = ROLES.iter().filter_map(|t| from_canonical_id(t.role_id)).collect();
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
        let want = [
            ("orchestrator", 13, 5),
            ("architect", 14, 4),
            ("builder", 11, 3),
            ("red-team", 9, 1),
            ("strategist", 10, 6),
            ("scout", 3, 3),
            // Spec 087's roles. Sixteen colours cannot match thirteen hues; these are the unused codes nearest in feel.
            ("administrator", 8, 8),
            ("assistant", 12, 4),
            ("advocate", 1, 1),
            ("messenger", 4, 4),
            ("validator", 5, 5),
            ("writer", 6, 6),
            ("investigator", 15, 8),
        ];
        for (role, ansi, ansi_light) in want {
            let t = ROLES.iter().find(|t| t.role_id == role).unwrap();
            assert_eq!(t.chassis_ansi, ansi, "{}", role);
            assert_eq!(t.chassis_ansi_light, ansi_light, "{}", role);
            assert_eq!(ui_spec()["roles"][role]["ansi"], ansi, "{}", role);
            assert_eq!(ui_spec()["roles"][role]["ansi_light"], ansi_light, "{}", role);
        }
        assert!(ROLES.iter().all(|t| t.chassis_ansi < 16 && t.chassis_ansi_light < 16));
        // On a dark 16-colour terminal no two roles share a code (ui/test/theme.test.ts holds the same line on the rendered styles).
        let codes: std::collections::HashSet<_> = ROLES.iter().map(|t| t.chassis_ansi).collect();
        assert_eq!(codes.len(), ROLES.len(), "two roles share a 16-colour code");
        // Code 2 is the console's `ok` green and 0 is invisible on a dark terminal (the red `alert` is bold, so code 1 is free).
        assert!(!codes.contains(&2) && !codes.contains(&0), "a role took the ok colour or black");
    }

    // ---- spec 087 §3 and §6.1 ----

    /// The core table of spec 087 §3.1, pinned: role id, idle face, blink face. The scout's three-frame blink is S4's; its
    /// first frame is `---` here.
    const CORE_FACES: [(&str, &str, &str); 13] = [
        ("orchestrator", "•─•", "-─-"),
        ("builder", "•=•", ">=<"),
        ("administrator", "•.•", "-.-"),
        ("architect", "•o•", "-o-"),
        ("strategist", "•u•", "-u-"),
        ("assistant", "•‿•", "-‿-"),
        ("advocate", "•^•", "*^*"),
        ("messenger", "•w•", "·w·"),
        ("validator", "¬_•", "-_-"),
        ("red-team", "¬_¬", "-_-"),
        ("writer", "•_•", "•_-"),
        ("investigator", "•_•", "-_•"),
        ("scout", "•••", "---"),
    ];

    #[test]
    fn get_theme_and_from_canonical_id_agree_for_every_role() {
        for t in ROLES.iter() {
            let role = from_canonical_id(t.role_id).unwrap_or_else(|| panic!("{} has no AgentRole", t.role_id));
            assert_eq!(get_theme(role).role_id, t.role_id);
        }
        let ids: std::collections::HashSet<_> = ROLES.iter().map(|t| t.role_id).collect();
        assert_eq!(ids.len(), ROLES.len(), "role ids are unique");
    }

    #[test]
    fn the_core_faces_are_the_ones_spec_087_names() {
        for (id, idle, blink) in CORE_FACES {
            let role = from_canonical_id(id).unwrap();
            assert_eq!(badge_cells(role, AgentActivity::Idle), idle, "{id} idle");
            assert_eq!(badge_cells(role, AgentActivity::Blink), blink, "{id} blink");
            assert_eq!(delphi_core::research::role_look(id).0, format!("⟦{idle}⟧"), "{id} outside badge");
        }
        // Every role of the spec's table is in the library, and the library has no core role the spec does not name.
        let named: std::collections::HashSet<_> = CORE_FACES.iter().map(|(id, _, _)| *id).collect();
        for t in ROLES.iter().filter(|t| t.role_id != "operator") {
            assert!(named.contains(t.role_id), "{} is in ROLES but not in spec 087 §3.1", t.role_id);
        }
    }

    #[test]
    fn a_blink_is_never_its_own_idle_face_and_faces_use_only_the_allowed_characters() {
        // Spec 087 §6.2: width-1 characters; the ambiguous-width ones are for pool entries only.
        const ALLOWED: &str = "•·°^_-─=.ouw~><*¬‿";
        for (id, idle, blink) in CORE_FACES {
            assert_ne!(idle, blink, "{id} blinks to itself");
            for face in [idle, blink] {
                assert_eq!(face.chars().count(), 3, "{id} {face}");
                assert!(face.chars().all(|c| ALLOWED.contains(c)), "{id} {face} uses a character outside the allowed set");
            }
        }
    }

    #[test]
    fn only_the_twins_share_an_idle_face() {
        let mut seen: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
        for (id, idle, _) in CORE_FACES {
            seen.entry(idle).or_default().push(id);
        }
        for (face, roles) in seen.iter().filter(|(_, r)| r.len() > 1) {
            let mut r = roles.clone();
            r.sort();
            assert_eq!(r, ["investigator", "writer"], "`{face}` is shared by {r:?}; only the twins may share a face");
        }
        // They are told apart by colour and by mirrored winks.
        let w = get_theme(AgentRole::Writer);
        let i = get_theme(AgentRole::Investigator);
        assert_ne!(w.chassis_rgb, i.chassis_rgb);
        assert_eq!((w.blink_badge, i.blink_badge), ("•_-", "-_•"));
    }

    fn lin(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    fn luminance(c: (u8, u8, u8)) -> f64 {
        0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
    }
    fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    fn lab(c: (u8, u8, u8)) -> (f64, f64, f64) {
        let (r, g, b) = (lin(c.0), lin(c.1), lin(c.2));
        let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
        let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
        (116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z)))
    }
    fn delta_e(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
        let (p, q) = (lab(a), lab(b));
        ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2) + (p.2 - q.2).powi(2)).sqrt()
    }

    /// The same gate as `ui/test/contrast.test.ts`, run on the library's own data: 3.0:1 on the three dark backgrounds
    /// for the chassis, 4.0:1 on white and solarized light for the light chassis (spec 087 §2 item 7). The new light chassis
    /// all reach 4.5:1.
    #[test]
    fn every_chassis_clears_the_contrast_gate_in_both_modes() {
        const DARK: [(u8, u8, u8); 3] = [(0, 0, 0), (15, 23, 42), (40, 42, 54)];
        const LIGHT: [(u8, u8, u8); 2] = [(255, 255, 255), (253, 246, 227)];
        let new_in_087 = ["administrator", "assistant", "advocate", "messenger", "validator", "writer", "investigator"];
        for t in ROLES.iter() {
            for bg in DARK {
                assert!(contrast(t.chassis_rgb, bg) >= 3.0, "{} dark chassis on {:?}: {:.2}", t.role_id, bg, contrast(t.chassis_rgb, bg));
            }
            let want = if new_in_087.contains(&t.role_id) { 4.5 } else { 4.0 };
            for bg in LIGHT {
                assert!(contrast(t.chassis_rgb_light, bg) >= want, "{} light chassis on {:?}: {:.2}", t.role_id, bg, contrast(t.chassis_rgb_light, bg));
            }
        }
    }

    /// No two roles' chassis colours are closer than they already were when spec 087 was written: the closest existing
    /// pairs were red-team/advocate (14.7 in CIELAB, dark) and administrator/operator (9.1, light). The floors are
    /// those measurements rounded down; they pin the spread so a later colour cannot quietly crowd another.
    #[test]
    fn no_two_chassis_colours_are_closer_than_the_measured_floor() {
        const MIN_DELTA_E_DARK: f64 = 14.0;
        const MIN_DELTA_E_LIGHT: f64 = 9.0;
        for (i, a) in ROLES.iter().enumerate() {
            for b in ROLES.iter().skip(i + 1) {
                let d = delta_e(a.chassis_rgb, b.chassis_rgb);
                assert!(d >= MIN_DELTA_E_DARK, "{} and {} are {:.1} apart on dark terminals", a.role_id, b.role_id, d);
                let l = delta_e(a.chassis_rgb_light, b.chassis_rgb_light);
                assert!(l >= MIN_DELTA_E_LIGHT, "{} and {} are {:.1} apart on light terminals", a.role_id, b.role_id, l);
            }
        }
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
                        let chassis = fg(if dark { t.chassis_rgb } else { t.chassis_rgb_light });
                        let mouth_ch = cells.chars().nth(m).unwrap();
                        assert!(out.contains(&format!("{chassis}{mouth_ch}")), "{} {} mouth is chassis-coloured", t.role_id, name);
                    }
                }
            }
        }
    }
}

//! Resolving role mentions and aliases (@athena, @orchestrator, @delphi) across themes (`specs/077`).
//!
//! Theme names exist purely for operator display and mention parsing.
//! In Delphi internals, only canonical role IDs (`orchestrator`, `architect`, etc.) are used (`[REV-ARCH-05]`).
//! This module resolves incoming mention tags to canonical role IDs at the system boundary.

use std::collections::HashMap;

pub const DEFAULT_THEME_TEXT: &str = include_str!("../../../themes/greek/theme.md");

/// Maps canonical role IDs to themed display names, and resolves mentions.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeResolver {
    /// role_id -> display_name (e.g. "orchestrator" -> "Athena")
    roles: HashMap<String, String>,
}

impl Default for ThemeResolver {
    fn default() -> Self {
        Self::from_theme_text(DEFAULT_THEME_TEXT)
    }
}

/// A mapping entry for roster display and discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMapping {
    pub role_id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
}

impl ThemeResolver {
    /// Create a resolver from a pre-constructed role-to-name mapping.
    pub fn new(roles: HashMap<String, String>) -> Self {
        Self { roles }
    }

    /// Parse a resolver directly from `theme.md` YAML text.
    /// Extracts `roles:\n  <role>: { name: "<Name>", ... }` or multi-line YAML.
    pub fn from_theme_text(text: &str) -> Self {
        let mut roles = HashMap::new();
        let mut in_roles = false;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("roles:") {
                in_roles = true;
                continue;
            }
            if in_roles {
                if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.is_empty() {
                    in_roles = false;
                    continue;
                }
                // Expecting e.g. `orchestrator: { name: "Athena", ... }` or `architect: { name: "Daedalus" ... }`
                if let Some(colon) = trimmed.find(':') {
                    let role_key = trimmed[..colon].trim().to_lowercase();
                    let rest = trimmed[colon + 1..].trim();
                    if let Some(name_idx) = rest.find("name:") {
                        let name_part = &rest[name_idx + 5..].trim_start();
                        if let Some(start_quote) = name_part.find('"') {
                            if let Some(end_quote) = name_part[start_quote + 1..].find('"') {
                                let name = &name_part[start_quote + 1..start_quote + 1 + end_quote];
                                if !name.is_empty() {
                                    roles.insert(role_key, name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        Self { roles }
    }

    /// Resolve an incoming mention tag (`@athena`, `@orchestrator`, `@delphi`, `Daedalus`, etc.) to a canonical role ID.
    ///
    /// - Strips any leading '@'.
    /// - Matches case-insensitively.
    /// - Special front-door mention: "delphi" resolves to "orchestrator".
    /// - Matches against canonical role IDs directly.
    /// - Matches against configured theme display names.
    /// - If no match is found, returns the normalized (stripped) tag as-is.
    pub fn resolve(&self, tag: &str) -> String {
        let clean = tag.strip_prefix('@').unwrap_or(tag).trim();
        let want = clean.to_lowercase();

        // 1. Front-door mention: @delphi -> orchestrator
        if want == "delphi" {
            return "orchestrator".to_string();
        }

        // 2. Direct canonical role ID match
        if self.roles.contains_key(&want) || is_canonical_role(&want) {
            return want;
        }

        // 3. Themed display name match (case-insensitive)
        for (id, display) in &self.roles {
            if display.to_lowercase() == want {
                return id.clone();
            }
        }

        clean.to_string()
    }

    /// The display name for a role, falling back to the role ID itself.
    pub fn display_name(&self, role: &str) -> String {
        let r = role.strip_prefix('@').unwrap_or(role).trim().to_lowercase();
        self.roles.get(&r).cloned().unwrap_or(r)
    }

    /// Aliases that point to this role: `@<display_name>` and `@<role_id>` (plus `@delphi` for orchestrator).
    pub fn aliases_for(&self, role: &str) -> Vec<String> {
        let r = role.strip_prefix('@').unwrap_or(role).trim().to_lowercase();
        let mut aliases = Vec::new();
        if let Some(name) = self.roles.get(&r) {
            aliases.push(format!("@{}", name.to_lowercase()));
        }
        aliases.push(format!("@{}", r));
        if r == "orchestrator" && !aliases.iter().any(|a| a == "@delphi") {
            aliases.push("@delphi".to_string());
        }
        aliases
    }

    /// Return all active role mappings for discovery rosters (@delphi roster).
    pub fn all_mappings(&self) -> Vec<RoleMapping> {
        let mut known_roles = vec![
            "orchestrator",
            "architect",
            "red-team",
            "builder",
            "strategist",
            "investigator",
            "scout",
            "writer",
            "messenger",
        ];
        // Include any additional roles present in self.roles
        for k in self.roles.keys() {
            if !known_roles.contains(&k.as_str()) {
                known_roles.push(k.as_str());
            }
        }

        known_roles
            .into_iter()
            .map(|r| RoleMapping {
                role_id: r.to_string(),
                display_name: self.display_name(r),
                aliases: self.aliases_for(r),
            })
            .collect()
    }

    /// Raw mapping access.
    pub fn inner_map(&self) -> &HashMap<String, String> {
        &self.roles
    }
}

/// Standalone resolve function matching `chat.rs` signature for backward compatibility.
pub fn resolve_role(names: &HashMap<String, String>, tag: &str) -> String {
    let resolver = ThemeResolver { roles: names.clone() };
    resolver.resolve(tag)
}

fn is_canonical_role(r: &str) -> bool {
    matches!(
        r,
        "orchestrator"
            | "architect"
            | "red-team"
            | "builder"
            | "strategist"
            | "investigator"
            | "scout"
            | "writer"
            | "messenger"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_theme() -> ThemeResolver {
        let text = r#"
name: "greek"
roles:
  orchestrator:  { name: "Athena",     epithet: "owl-eyed counselor" }
  architect:     { name: "Daedalus",   epithet: "cunning artificer" }
  red-team:      { name: "Ares",       epithet: "stormer of walls" }
  builder:       { name: "Hephaestus", epithet: "limping smith" }
  strategist:    { name: "Odysseus",   epithet: "of many turns" }
"#;
        ThemeResolver::from_theme_text(text)
    }

    #[test]
    fn test_resolves_canonical_roles_with_and_without_at() {
        let res = sample_theme();
        assert_eq!(res.resolve("architect"), "architect");
        assert_eq!(res.resolve("@architect"), "architect");
        assert_eq!(res.resolve("ARCHITECT"), "architect");
        assert_eq!(res.resolve("@red-team"), "red-team");
    }

    #[test]
    fn test_resolves_theme_display_names() {
        let res = sample_theme();
        assert_eq!(res.resolve("Athena"), "orchestrator");
        assert_eq!(res.resolve("@athena"), "orchestrator");
        assert_eq!(res.resolve("@Daedalus"), "architect");
        assert_eq!(res.resolve("ares"), "red-team");
        assert_eq!(res.resolve("@hephaestus"), "builder");
        assert_eq!(res.resolve("@Odysseus"), "strategist");
    }

    #[test]
    fn test_resolves_delphi_front_door_to_orchestrator() {
        let res = sample_theme();
        assert_eq!(res.resolve("@delphi"), "orchestrator");
        assert_eq!(res.resolve("delphi"), "orchestrator");
        assert_eq!(res.resolve("DELPHI"), "orchestrator");
    }

    #[test]
    fn test_aliases_and_display_name() {
        let res = sample_theme();
        assert_eq!(res.display_name("orchestrator"), "Athena");
        assert_eq!(res.display_name("architect"), "Daedalus");
        assert_eq!(res.display_name("unknown"), "unknown");

        let aliases = res.aliases_for("orchestrator");
        assert!(aliases.contains(&"@athena".to_string()));
        assert!(aliases.contains(&"@orchestrator".to_string()));
        assert!(aliases.contains(&"@delphi".to_string()));
    }

    #[test]
    fn test_unknown_tag_passthrough() {
        let res = sample_theme();
        assert_eq!(res.resolve("@someone_else"), "someone_else");
        assert_eq!(res.resolve("nobody"), "nobody");
    }
}

//! User configuration: a standalone TOML file, validated on load.
//!
//! The daemon owns this file (`riftctl` only triggers a reload over IPC). A
//! missing file is not an error — the built-in defaults match
//! [`crate::layout::LayoutParams::default`], so rift runs unconfigured. A file
//! that is present but malformed or out of range is rejected wholesale with a
//! diagnostic; the daemon never applies a partial configuration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, bail};
use rift_ipc::LayoutKind;
use serde::Deserialize;

use crate::keys;

/// The effective configuration, mirroring the `riftrc` sections.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub layout: LayoutConfig,
    pub gaps: GapsConfig,
    pub behavior: BehaviorConfig,
    /// `[[rules]]`: per-window overrides matched on class/title.
    pub rules: Vec<WindowRule>,
    /// `[keys]`: per-binding key overrides, keyed by the stable binding id
    /// (e.g. `rift_focus_left = "Meta+Left"`). Unknown ids are rejected.
    pub keys: HashMap<String, String>,
}

/// A `[[rules]]` entry: match a window by `class` and/or `title` and apply an
/// override. A rule with neither field set matches nothing (and is rejected by
/// [`Config::validate`]); when both are set, both must match.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowRule {
    /// Substring matched (case-sensitively) against the window's resource class.
    #[serde(default)]
    pub class: Option<String>,
    /// Substring matched (case-sensitively) against the window's title.
    #[serde(default)]
    pub title: Option<String>,
    /// Force the matched window to float (skip tiling).
    #[serde(default)]
    pub float: bool,
}

impl WindowRule {
    /// Whether this rule matches a window's `class`/`title`. Each present field
    /// is a case-sensitive substring test; an absent field is a wildcard. A rule
    /// with no fields never matches (guarded against in [`Config::validate`]).
    pub fn matches(&self, class: Option<&str>, title: Option<&str>) -> bool {
        if self.class.is_none() && self.title.is_none() {
            return false;
        }
        let class_ok = match &self.class {
            Some(want) => class.is_some_and(|c| c.contains(want.as_str())),
            None => true,
        };
        let title_ok = match &self.title {
            Some(want) => title.is_some_and(|t| t.contains(want.as_str())),
            None => true,
        };
        class_ok && title_ok
    }
}

/// `[layout]`: the default layout and its master-region tunables.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    /// Layout assigned to newly materialized cells.
    #[serde(deserialize_with = "de_layout_kind")]
    pub default: LayoutKind,
    /// Fraction of the area given to the master region.
    pub master_ratio: f32,
    /// Number of windows in the master region.
    pub master_count: usize,
    /// Whether global auto-tiling starts enabled.
    pub tiling_enabled: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            default: LayoutKind::default(),
            master_ratio: 0.6,
            master_count: 1,
            tiling_enabled: true,
        }
    }
}

/// `[gaps]`: spacing between tiles and around the screen edge, in pixels.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GapsConfig {
    pub inner: i32,
    pub outer: i32,
}

impl Default for GapsConfig {
    fn default() -> Self {
        Self {
            inner: 8,
            outer: 12,
        }
    }
}

/// `[behavior]`: session behavior flags. Parsed and surfaced now; their runtime
/// effects are deferred (see the milestone notes).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BehaviorConfig {
    pub per_desktop: bool,
    pub per_activity: bool,
    pub focus_follows_mouse: bool,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            per_desktop: true,
            per_activity: true,
            focus_follows_mouse: false,
        }
    }
}

/// Deserialize a lowercase layout name (e.g. `"tile"`) into a [`LayoutKind`].
fn de_layout_kind<'de, D>(deserializer: D) -> Result<LayoutKind, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    LayoutKind::from_str(&s).map_err(serde::de::Error::custom)
}

impl Config {
    /// Load and validate the config at `path`.
    ///
    /// A missing file yields the built-in defaults. A present file is parsed and
    /// validated; any error (I/O, parse, or range) is returned with context and
    /// nothing is applied.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(e).with_context(|| format!("reading config {}", path.display()));
            }
        };
        let config: Self =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        config
            .validate()
            .with_context(|| format!("invalid config {}", path.display()))?;
        Ok(config)
    }

    /// Reject out-of-range values so a bad config never partially applies.
    pub fn validate(&self) -> anyhow::Result<()> {
        let ratio = self.layout.master_ratio;
        if !ratio.is_finite() || !(0.05..=0.95).contains(&ratio) {
            bail!("layout.master_ratio must be between 0.05 and 0.95 (got {ratio})");
        }
        if self.layout.master_count < 1 {
            bail!("layout.master_count must be at least 1");
        }
        if self.gaps.inner < 0 {
            bail!("gaps.inner must not be negative (got {})", self.gaps.inner);
        }
        if self.gaps.outer < 0 {
            bail!("gaps.outer must not be negative (got {})", self.gaps.outer);
        }
        for (i, rule) in self.rules.iter().enumerate() {
            if rule.class.is_none() && rule.title.is_none() {
                bail!("rules[{i}] must set at least one of `class` or `title`");
            }
        }
        for (id, key) in &self.keys {
            if !keys::is_known_id(id) {
                bail!("keys.{id} is not a known binding id");
            }
            if key.trim().is_empty() {
                bail!("keys.{id} must not be empty");
            }
        }
        Ok(())
    }
}

/// Resolve the default config path.
///
/// Uses `$XDG_CONFIG_HOME/riftrc`, falling back to `$HOME/.config/riftrc`.
pub fn default_config_path() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".config")
        });
    config_home.join("riftrc")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(body: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f
    }

    #[test]
    fn full_config_parses() {
        let f = write_tmp(
            r#"
            [layout]
            default = "spiral"
            master_ratio = 0.55
            master_count = 2
            [gaps]
            inner = 4
            outer = 16
            [behavior]
            per_desktop = false
            per_activity = false
            focus_follows_mouse = true
            "#,
        );
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.layout.default, LayoutKind::Spiral);
        assert_eq!(cfg.layout.master_ratio, 0.55);
        assert_eq!(cfg.layout.master_count, 2);
        assert_eq!(cfg.gaps.inner, 4);
        assert_eq!(cfg.gaps.outer, 16);
        assert!(!cfg.behavior.per_desktop);
        assert!(cfg.behavior.focus_follows_mouse);
    }

    #[test]
    fn tiling_enabled_defaults_true_and_can_be_disabled() {
        assert!(LayoutConfig::default().tiling_enabled);
        let f = write_tmp("[layout]\ntiling_enabled = false\n");
        let cfg = Config::load(f.path()).unwrap();
        assert!(!cfg.layout.tiling_enabled);
    }

    #[test]
    fn missing_file_is_defaults() {
        let cfg = Config::load(Path::new("/nonexistent/rift/riftrc")).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn partial_sections_keep_defaults() {
        let f = write_tmp("[gaps]\ninner = 0\n");
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.gaps.inner, 0);
        assert_eq!(cfg.gaps.outer, 12); // default
        assert_eq!(cfg.layout.default, LayoutKind::Tile); // default
        assert!(cfg.behavior.per_desktop); // default
    }

    #[test]
    fn unknown_key_is_rejected() {
        let f = write_tmp("[layout]\nbogus = 1\n");
        assert!(Config::load(f.path()).is_err());
    }

    #[test]
    fn bad_layout_name_is_rejected() {
        let f = write_tmp("[layout]\ndefault = \"grid\"\n");
        assert!(Config::load(f.path()).is_err());
    }

    #[test]
    fn out_of_range_ratio_is_rejected() {
        let f = write_tmp("[layout]\nmaster_ratio = 9.0\n");
        assert!(Config::load(f.path()).is_err());
    }

    #[test]
    fn zero_master_count_is_rejected() {
        let f = write_tmp("[layout]\nmaster_count = 0\n");
        assert!(Config::load(f.path()).is_err());
    }

    #[test]
    fn window_rules_parse() {
        let f = write_tmp(
            r#"
            [[rules]]
            class = "pavucontrol"
            float = true
            [[rules]]
            title = "Picture-in-Picture"
            float = true
            "#,
        );
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.rules.len(), 2);
        assert_eq!(cfg.rules[0].class.as_deref(), Some("pavucontrol"));
        assert!(cfg.rules[0].float);
        assert_eq!(cfg.rules[1].title.as_deref(), Some("Picture-in-Picture"));
    }

    #[test]
    fn rule_without_match_field_is_rejected() {
        let f = write_tmp("[[rules]]\nfloat = true\n");
        assert!(Config::load(f.path()).is_err());
    }

    #[test]
    fn key_overrides_parse() {
        let f = write_tmp(
            r#"
            [keys]
            rift_focus_left = "Meta+Left"
            rift_layout_tile = "Meta+T"
            "#,
        );
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(
            cfg.keys.get("rift_focus_left").map(String::as_str),
            Some("Meta+Left")
        );
        assert_eq!(
            cfg.keys.get("rift_layout_tile").map(String::as_str),
            Some("Meta+T")
        );
    }

    #[test]
    fn unknown_key_id_is_rejected() {
        let f = write_tmp("[keys]\nrift_focus_sideways = \"Meta+X\"\n");
        assert!(Config::load(f.path()).is_err());
    }

    #[test]
    fn empty_key_override_is_rejected() {
        let f = write_tmp("[keys]\nrift_focus_left = \"\"\n");
        assert!(Config::load(f.path()).is_err());
    }

    #[test]
    fn rule_matches_on_class_and_title() {
        let class_only = WindowRule {
            class: Some("kitty".into()),
            title: None,
            float: true,
        };
        assert!(class_only.matches(Some("kitty"), None));
        assert!(class_only.matches(Some("org.kitty"), Some("anything")));
        assert!(!class_only.matches(Some("konsole"), None));
        assert!(!class_only.matches(None, None));

        let both = WindowRule {
            class: Some("firefox".into()),
            title: Some("Picture-in-Picture".into()),
            float: true,
        };
        assert!(both.matches(Some("firefox"), Some("Picture-in-Picture")));
        assert!(!both.matches(Some("firefox"), Some("Mozilla Firefox")));
    }
}

use crate::git_info::resolve_root_git_project_for_trust;
use anyhow::Context as _;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::TUI_VISIBLE_COLLABORATION_MODES;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleKind {
    Primary,
    Spawnable,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleDefinition {
    pub id: String,
    pub plugin_id: String,
    pub kind: RoleKind,
    pub label: String,
    pub description: Option<String>,
    pub prompt_path: PathBuf,
    pub read_only: bool,
    pub allows_collab_tools: bool,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryDefinition {
    pub id: String,
    pub plugin_id: String,
    pub label: String,
    pub prompt_append_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiSpec {
    pub tab_picker_roles: Vec<String>,
    pub agent_config_callers: Vec<String>,
    pub agent_config_targets: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginRegistry {
    enabled: Vec<String>,
    default_agent_role: Option<String>,
    roles: BTreeMap<String, RoleDefinition>,
    categories: BTreeMap<String, CategoryDefinition>,
    collaboration_mode_overrides: HashMap<ModeKind, String>,
    spawn_defaults_allow: HashMap<String, Vec<String>>,
    ui: UiSpec,
}

impl PluginRegistry {
    pub fn enabled_plugin_ids(&self) -> &[String] {
        &self.enabled
    }

    pub fn is_empty(&self) -> bool {
        self.enabled.is_empty()
            && self.roles.is_empty()
            && self.categories.is_empty()
            && self.collaboration_mode_overrides.is_empty()
            && self.spawn_defaults_allow.is_empty()
            && self.ui.tab_picker_roles.is_empty()
            && self.ui.agent_config_callers.is_empty()
            && self.ui.agent_config_targets.is_empty()
    }

    pub fn default_agent_role(&self) -> Option<&str> {
        self.default_agent_role.as_deref()
    }

    pub fn roles(&self) -> impl Iterator<Item = &RoleDefinition> {
        self.roles.values()
    }

    pub fn role(&self, id: &str) -> Option<&RoleDefinition> {
        self.roles.get(&normalize_id(id))
    }

    pub fn spawnable_roles(&self) -> impl Iterator<Item = &RoleDefinition> {
        self.roles
            .values()
            .filter(|role| role.kind == RoleKind::Spawnable)
    }

    pub fn categories(&self) -> impl Iterator<Item = &CategoryDefinition> {
        self.categories.values()
    }

    pub fn category(&self, id: &str) -> Option<&CategoryDefinition> {
        self.categories.get(&normalize_id(id))
    }

    pub fn collaboration_mode_override(&self, mode: ModeKind) -> Option<&str> {
        self.collaboration_mode_overrides
            .get(&mode)
            .map(String::as_str)
    }

    pub fn spawn_defaults_allow(&self) -> &HashMap<String, Vec<String>> {
        &self.spawn_defaults_allow
    }

    pub fn ui(&self) -> &UiSpec {
        &self.ui
    }

    pub fn read_role_prompt(&self, role_id: &str) -> anyhow::Result<String> {
        let role = self
            .role(role_id)
            .with_context(|| format!("unknown role `{role_id}`"))?;
        fs::read_to_string(&role.prompt_path)
            .with_context(|| format!("failed to read role prompt {}", role.prompt_path.display()))
    }

    pub fn read_category_prompt_append(&self, category_id: &str) -> anyhow::Result<String> {
        let category = self
            .category(category_id)
            .with_context(|| format!("unknown category `{category_id}`"))?;
        fs::read_to_string(&category.prompt_append_path).with_context(|| {
            format!(
                "failed to read category prompt append {}",
                category.prompt_append_path.display()
            )
        })
    }

    /// Load and merge plugin manifests from default search locations.
    ///
    /// Enabled plugins are specified by `enabled` and searched for under:
    /// - `$CODEX_HOME/plugins/<plugin-id>/plugin.toml`
    /// - `<repo-root>/plugins/<plugin-id>/plugin.toml` (if cwd is in a git repo)
    /// - `<cwd>/plugins/<plugin-id>/plugin.toml` (fallback if no git repo)
    pub fn load(enabled: &[String], cwd: &Path, codex_home: &Path) -> anyhow::Result<Self> {
        let enabled = enabled
            .iter()
            .map(|id| normalize_id(id))
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            return Ok(Self::default());
        }
        let enabled_set: HashSet<String> = enabled.iter().cloned().collect();

        let search_dirs = plugin_search_dirs(cwd, codex_home);
        let mut manifests = HashMap::<String, (PathBuf, PluginManifest)>::new();

        for dir in &search_dirs {
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };
                let plugin_dir = entry.path();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if !is_dir {
                    continue;
                }
                let manifest_path = plugin_dir.join("plugin.toml");
                if !manifest_path.is_file() {
                    continue;
                }

                let raw = fs::read_to_string(&manifest_path).with_context(|| {
                    format!("failed to read plugin manifest {}", manifest_path.display())
                })?;
                let manifest: PluginManifest = toml::from_str(&raw).with_context(|| {
                    format!(
                        "failed to parse plugin manifest {}",
                        manifest_path.display()
                    )
                })?;

                let plugin_id = normalize_id(&manifest.plugin.id);
                if !enabled_set.contains(&plugin_id) {
                    continue;
                }
                if manifests.contains_key(&plugin_id) {
                    anyhow::bail!(
                        "duplicate plugin `{plugin_id}` found (searched: {})",
                        search_dirs
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                manifests.insert(plugin_id, (plugin_dir, manifest));
            }
        }

        let missing = enabled
            .iter()
            .filter(|id| !manifests.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            anyhow::bail!(
                "enabled plugin(s) not found: {} (searched: {})",
                missing.join(", "),
                search_dirs
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        let mut registry = Self {
            enabled: enabled.clone(),
            ..Default::default()
        };

        for plugin_id in &enabled {
            let (plugin_dir, manifest) = manifests
                .get(plugin_id)
                .with_context(|| format!("missing manifest for plugin `{plugin_id}`"))?
                .clone();

            if registry.default_agent_role.is_none() {
                registry.default_agent_role = manifest
                    .plugin
                    .default_agent_role
                    .as_deref()
                    .map(normalize_id)
                    .filter(|id| !id.is_empty());
            }

            for (mode, override_manifest) in manifest.collaboration_modes {
                if registry.collaboration_mode_overrides.contains_key(&mode) {
                    anyhow::bail!(
                        "duplicate collaboration mode override `{mode:?}` across enabled plugins"
                    );
                }
                let prompt_path = resolve_plugin_path(&plugin_dir, &override_manifest.prompt_file)
                    .with_context(|| {
                        format!("invalid prompt_file for collaboration mode override `{mode:?}`")
                    })?;
                let raw = fs::read_to_string(&prompt_path).with_context(|| {
                    format!(
                        "failed to read collaboration mode override prompt {}",
                        prompt_path.display()
                    )
                })?;
                if raw.trim().is_empty() {
                    anyhow::bail!(
                        "collaboration mode override prompt is empty: {}",
                        prompt_path.display()
                    );
                }
                let rendered = render_collaboration_mode_template(&raw, mode);
                if rendered.trim().is_empty() {
                    anyhow::bail!(
                        "collaboration mode override prompt rendered empty: {}",
                        prompt_path.display()
                    );
                }
                registry.collaboration_mode_overrides.insert(mode, rendered);
            }

            for (raw_role_id, role) in manifest.roles {
                let role_id = normalize_id(&raw_role_id);
                if role_id.is_empty() {
                    continue;
                }
                if registry.roles.contains_key(&role_id) {
                    anyhow::bail!("duplicate role id `{role_id}` across enabled plugins");
                }

                let prompt_path = resolve_plugin_path(&plugin_dir, &role.prompt_file)
                    .with_context(|| format!("invalid prompt_file for role `{raw_role_id}`"))?;
                let label = role.label.unwrap_or_else(|| role_id.clone());
                registry.roles.insert(
                    role_id.clone(),
                    RoleDefinition {
                        id: role_id,
                        plugin_id: plugin_id.clone(),
                        kind: role.kind,
                        label,
                        description: role.description,
                        prompt_path,
                        read_only: role.read_only.unwrap_or(false),
                        allows_collab_tools: role.allows_collab_tools.unwrap_or(true),
                        model: role.model,
                        reasoning_effort: role.reasoning_effort,
                    },
                );
            }

            for (raw_category_id, category) in manifest.categories {
                let category_id = normalize_id(&raw_category_id);
                if category_id.is_empty() {
                    continue;
                }
                if registry.categories.contains_key(&category_id) {
                    anyhow::bail!("duplicate category id `{category_id}` across enabled plugins");
                }
                let prompt_append_path =
                    resolve_plugin_path(&plugin_dir, &category.prompt_append_file).with_context(
                        || format!("invalid prompt_append_file for category `{raw_category_id}`"),
                    )?;
                registry.categories.insert(
                    category_id.clone(),
                    CategoryDefinition {
                        id: category_id,
                        plugin_id: plugin_id.clone(),
                        label: category.label.unwrap_or_else(|| raw_category_id.clone()),
                        prompt_append_path,
                    },
                );
            }

            if !manifest.spawn_defaults.allow.is_empty() {
                for (caller, targets) in manifest.spawn_defaults.allow {
                    registry
                        .spawn_defaults_allow
                        .insert(normalize_id(&caller), targets);
                }
            }

            registry.ui.tab_picker_roles.extend(
                manifest
                    .ui
                    .tab_picker_roles
                    .into_iter()
                    .map(|id| normalize_id(&id))
                    .filter(|id| !id.is_empty()),
            );
            registry.ui.agent_config_callers.extend(
                manifest
                    .ui
                    .agent_config_callers
                    .into_iter()
                    .map(|id| normalize_id(&id))
                    .filter(|id| !id.is_empty()),
            );
            registry.ui.agent_config_targets.extend(
                manifest
                    .ui
                    .agent_config_targets
                    .into_iter()
                    .map(|id| normalize_id(&id))
                    .filter(|id| !id.is_empty()),
            );
        }

        Ok(registry)
    }
}

const KNOWN_MODE_NAMES_PLACEHOLDER: &str = "{{KNOWN_MODE_NAMES}}";
const REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER: &str = "{{REQUEST_USER_INPUT_AVAILABILITY}}";

fn render_collaboration_mode_template(template: &str, mode: ModeKind) -> String {
    let known_mode_names = format_mode_names(&TUI_VISIBLE_COLLABORATION_MODES);
    let request_user_input_availability = request_user_input_availability_message(mode);
    template
        .replace(KNOWN_MODE_NAMES_PLACEHOLDER, &known_mode_names)
        .replace(
            REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER,
            &request_user_input_availability,
        )
}

fn format_mode_names(modes: &[ModeKind]) -> String {
    let mode_names: Vec<&str> = modes.iter().map(|mode| mode.display_name()).collect();
    match mode_names.as_slice() {
        [] => "none".to_string(),
        [mode_name] => (*mode_name).to_string(),
        [first, second] => format!("{first} and {second}"),
        [..] => mode_names.join(", "),
    }
}

fn request_user_input_availability_message(mode: ModeKind) -> String {
    let mode_name = mode.display_name();
    if mode.allows_request_user_input() {
        format!("The `request_user_input` tool is available in {mode_name} mode.")
    } else {
        format!(
            "The `request_user_input` tool is unavailable in {mode_name} mode. If you call it while in {mode_name} mode, it will return an error."
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginManifest {
    plugin: PluginInfo,
    #[serde(default)]
    collaboration_modes: HashMap<ModeKind, CollaborationModeOverrideManifest>,
    #[serde(default)]
    roles: BTreeMap<String, RoleManifest>,
    #[serde(default)]
    categories: BTreeMap<String, CategoryManifest>,
    #[serde(default)]
    ui: UiManifest,
    #[serde(default)]
    spawn_defaults: SpawnDefaultsManifest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginInfo {
    id: String,
    #[serde(default)]
    default_agent_role: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleManifest {
    kind: RoleKind,
    label: Option<String>,
    description: Option<String>,
    prompt_file: String,
    #[serde(default)]
    read_only: Option<bool>,
    #[serde(default)]
    allows_collab_tools: Option<bool>,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationModeOverrideManifest {
    prompt_file: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CategoryManifest {
    label: Option<String>,
    prompt_append_file: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiManifest {
    #[serde(default)]
    tab_picker_roles: Vec<String>,
    #[serde(default)]
    agent_config_callers: Vec<String>,
    #[serde(default)]
    agent_config_targets: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnDefaultsManifest {
    #[serde(default)]
    allow: HashMap<String, Vec<String>>,
}

fn plugin_search_dirs(cwd: &Path, codex_home: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.push(codex_home.join("plugins"));
    match resolve_root_git_project_for_trust(cwd) {
        Some(repo_root) => dirs.push(repo_root.join("plugins")),
        None => dirs.push(cwd.join("plugins")),
    }

    let mut seen = HashSet::<PathBuf>::new();
    dirs.into_iter()
        .filter_map(|p| {
            let normalized = dunce::simplified(&p).to_path_buf();
            if seen.insert(normalized.clone()) {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
}

fn resolve_plugin_path(plugin_dir: &Path, raw: &str) -> anyhow::Result<PathBuf> {
    let rel = PathBuf::from(raw);
    if rel.is_absolute() {
        anyhow::bail!("path must be relative: {raw}");
    }
    for component in rel.components() {
        if matches!(component, Component::ParentDir) {
            anyhow::bail!("path must not contain '..': {raw}");
        }
    }
    Ok(plugin_dir.join(rel))
}

fn normalize_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn collaboration_mode_override_renders_placeholders() {
        let codex_home = tempfile::tempdir().expect("create temp codex home");
        let plugin_dir = codex_home.path().join("plugins").join("collab-prompts");
        fs::create_dir_all(plugin_dir.join("modes")).expect("create plugin dirs");

        fs::write(
            plugin_dir.join("plugin.toml"),
            r#"
[plugin]
id = "collab-prompts"

[collaboration_modes.heavy_plan]
prompt_file = "modes/heavy_plan.md"
"#,
        )
        .expect("write plugin.toml");

        fs::write(
            plugin_dir.join("modes/heavy_plan.md"),
            "Known mode names are {{KNOWN_MODE_NAMES}}.\n\n{{REQUEST_USER_INPUT_AVAILABILITY}}\n",
        )
        .expect("write mode prompt");

        let plugins = PluginRegistry::load(
            &["collab-prompts".to_string()],
            codex_home.path(),
            codex_home.path(),
        )
        .expect("load plugins");

        let override_text = plugins
            .collaboration_mode_override(ModeKind::HeavyPlan)
            .expect("override should exist");

        assert!(override_text.contains("Known mode names are Default, Plan, Heavy, Heavy Plan."));
        assert_eq!(
            true,
            override_text
                .contains("The `request_user_input` tool is available in Heavy Plan mode.")
        );
    }
}

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Widget as _;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::key_hint;
use crate::render::RectExt as _;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;

use codex_core::plugins::PluginRegistry;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;
use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::measure_rows_height;
use super::selection_popup_common::menu_surface_padding_height;
use super::selection_popup_common::render_menu_surface;
use super::selection_popup_common::render_rows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusPanel {
    Callers,
    Targets,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OverridePermission {
    All,
    AllowList(BTreeSet<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RoleSpec {
    id: String,
    label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EffectivePermission {
    All,
    AllowList(BTreeSet<String>),
    None,
}

impl EffectivePermission {
    fn allows(&self, role: &str) -> bool {
        match self {
            Self::All => true,
            Self::AllowList(allowed) => allowed.contains(role),
            Self::None => false,
        }
    }
}

pub(crate) struct AgentSpawnConfigView {
    focus: FocusPanel,
    complete: bool,

    callers: Vec<RoleSpec>,
    targets: Vec<RoleSpec>,
    all_targets_set: BTreeSet<String>,
    default_permissions: BTreeMap<String, EffectivePermission>,

    caller_state: ScrollState,
    target_state: ScrollState,

    overrides: BTreeMap<String, OverridePermission>,
    initial_overrides: BTreeMap<String, OverridePermission>,

    app_event_tx: AppEventSender,
    header: Box<dyn Renderable>,
    footer_hint: Line<'static>,
}

impl AgentSpawnConfigView {
    pub(crate) fn new(
        config_path: PathBuf,
        active_agent: String,
        allow_overrides: &HashMap<String, Vec<String>>,
        plugins: &PluginRegistry,
        app_event_tx: AppEventSender,
    ) -> Self {
        let callers = Self::build_role_specs(&plugins.ui().agent_config_callers, plugins);
        let targets = Self::build_role_specs(&plugins.ui().agent_config_targets, plugins);
        let caller_ids: BTreeSet<String> = callers.iter().map(|role| role.id.clone()).collect();
        let all_targets_set: BTreeSet<String> =
            targets.iter().map(|role| role.id.clone()).collect();
        let default_permissions =
            Self::build_default_permissions(plugins, &caller_ids, &all_targets_set);

        let (initial_overrides, warnings) =
            Self::parse_overrides(allow_overrides, &caller_ids, &all_targets_set);

        let mut header = ColumnRenderable::new();
        header.push(Line::from("Agent spawn permissions".bold()));
        header.push(Line::from(vec![
            "Config: ".dim(),
            config_path.display().to_string().into(),
        ]));
        header.push(Line::from(vec![
            "Active agent: ".dim(),
            active_agent.into(),
        ]));
        if !warnings.is_empty() {
            header.push(Line::from("Warnings (ignored):".dim()));
            for warning in &warnings {
                header.push(Line::from(vec!["- ".dim(), warning.clone().into()]));
            }
        }

        let mut view = Self {
            focus: FocusPanel::Callers,
            complete: false,
            callers,
            targets,
            all_targets_set,
            default_permissions,
            caller_state: ScrollState::new(),
            target_state: ScrollState::new(),
            overrides: initial_overrides.clone(),
            initial_overrides,
            app_event_tx,
            header: Box::new(header),
            footer_hint: agent_spawn_popup_hint_line(),
        };
        view.initialize_selection();
        view
    }

    fn build_role_specs(ids: &[String], plugins: &PluginRegistry) -> Vec<RoleSpec> {
        let mut specs = Vec::new();
        let mut seen = BTreeSet::<String>::new();
        for id in ids {
            let Some(role) = plugins.role(id) else {
                continue;
            };
            if !seen.insert(role.id.clone()) {
                continue;
            }
            specs.push(RoleSpec {
                id: role.id.clone(),
                label: role.label.clone(),
            });
        }
        specs
    }

    fn build_default_permissions(
        plugins: &PluginRegistry,
        caller_ids: &BTreeSet<String>,
        target_ids: &BTreeSet<String>,
    ) -> BTreeMap<String, EffectivePermission> {
        let mut defaults = BTreeMap::new();
        for caller in caller_ids {
            let Some(allowed) = plugins.spawn_defaults_allow().get(caller) else {
                defaults.insert(caller.clone(), EffectivePermission::None);
                continue;
            };
            if allowed.iter().any(|raw| matches_allow_all_token(raw)) {
                defaults.insert(caller.clone(), EffectivePermission::All);
                continue;
            }
            let allow_list = allowed
                .iter()
                .map(|raw| normalize_role_id(raw))
                .filter(|id| target_ids.contains(id))
                .collect::<BTreeSet<_>>();
            defaults.insert(
                caller.clone(),
                if allow_list.is_empty() {
                    EffectivePermission::None
                } else {
                    EffectivePermission::AllowList(allow_list)
                },
            );
        }
        defaults
    }

    fn parse_overrides(
        allow_overrides: &HashMap<String, Vec<String>>,
        caller_ids: &BTreeSet<String>,
        target_ids: &BTreeSet<String>,
    ) -> (BTreeMap<String, OverridePermission>, Vec<String>) {
        let mut overrides = BTreeMap::new();
        let mut warnings = Vec::new();

        for (caller_raw, allowed_raw) in allow_overrides {
            let caller = normalize_role_id(caller_raw);
            if !caller_ids.contains(&caller) {
                warnings.push(format!("Unknown caller role: {caller_raw}"));
                continue;
            }

            if allowed_raw.iter().any(|raw| matches_allow_all_token(raw)) {
                overrides.insert(caller, OverridePermission::All);
                continue;
            }

            let mut allow_list = BTreeSet::new();
            let mut unknown_targets = Vec::new();
            for raw in allowed_raw {
                let target = normalize_role_id(raw);
                if target_ids.contains(&target) {
                    allow_list.insert(target);
                } else {
                    unknown_targets.push(raw.clone());
                }
            }
            if !unknown_targets.is_empty() {
                warnings.push(format!(
                    "Unknown targets for {caller_raw}: {}",
                    unknown_targets.join(", ")
                ));
            }
            overrides.insert(caller, OverridePermission::AllowList(allow_list));
        }

        (overrides, warnings)
    }

    fn initialize_selection(&mut self) {
        if self.callers.is_empty() {
            self.caller_state.selected_idx = None;
            self.target_state.selected_idx = None;
        } else {
            self.caller_state.selected_idx = Some(0);
            self.target_state.selected_idx = (!self.targets.is_empty()).then_some(0);
        }
    }

    fn selected_caller_id(&self) -> Option<&str> {
        let idx = self.caller_state.selected_idx?;
        self.callers.get(idx).map(|role| role.id.as_str())
    }

    fn default_permission(&self, caller: &str) -> EffectivePermission {
        self.default_permissions
            .get(caller)
            .cloned()
            .unwrap_or(EffectivePermission::None)
    }

    fn effective_permission(&self, caller: &str) -> (EffectivePermission, &'static str) {
        let Some(override_permission) = self.overrides.get(caller) else {
            return (self.default_permission(caller), "default");
        };

        let effective = match override_permission {
            OverridePermission::All => EffectivePermission::All,
            OverridePermission::AllowList(allowed) if allowed.is_empty() => {
                EffectivePermission::None
            }
            OverridePermission::AllowList(allowed) => {
                EffectivePermission::AllowList(allowed.clone())
            }
        };
        (effective, "override")
    }

    fn describe_permission(permission: &EffectivePermission) -> String {
        match permission {
            EffectivePermission::All => "*".to_string(),
            EffectivePermission::None => "(none)".to_string(),
            EffectivePermission::AllowList(allowed) if allowed.is_empty() => "(none)".to_string(),
            EffectivePermission::AllowList(allowed) => {
                allowed.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        }
    }

    fn build_caller_rows(&self) -> Vec<GenericDisplayRow> {
        let mut rows = Vec::with_capacity(self.callers.len());
        for (idx, role) in self.callers.iter().enumerate() {
            let prefix = if self.caller_state.selected_idx == Some(idx) {
                '›'
            } else {
                ' '
            };
            let (permission, origin) = self.effective_permission(role.id.as_str());
            let description = format!("{origin} · {}", Self::describe_permission(&permission));
            rows.push(GenericDisplayRow {
                name: format!("{prefix} {}", role.label),
                description: Some(description),
                ..Default::default()
            });
        }
        rows
    }

    fn build_target_rows(&self) -> Vec<GenericDisplayRow> {
        let Some(caller) = self.selected_caller_id() else {
            return Vec::new();
        };
        let (permission, _) = self.effective_permission(caller);

        let mut rows = Vec::with_capacity(self.targets.len());
        for (idx, role) in self.targets.iter().enumerate() {
            let prefix = if self.target_state.selected_idx == Some(idx) {
                '›'
            } else {
                ' '
            };
            let marker = if permission.allows(role.id.as_str()) {
                'x'
            } else {
                ' '
            };
            rows.push(GenericDisplayRow {
                name: format!("{prefix} [{marker}] {}", role.label),
                ..Default::default()
            });
        }
        rows
    }

    fn move_up(&mut self) {
        match self.focus {
            FocusPanel::Callers => {
                let len = self.callers.len();
                self.caller_state.move_up_wrap(len);
                self.caller_state
                    .ensure_visible(len, MAX_POPUP_ROWS.min(len));
            }
            FocusPanel::Targets => {
                let len = self.targets.len();
                self.target_state.move_up_wrap(len);
                self.target_state
                    .ensure_visible(len, MAX_POPUP_ROWS.min(len));
            }
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            FocusPanel::Callers => {
                let len = self.callers.len();
                self.caller_state.move_down_wrap(len);
                self.caller_state
                    .ensure_visible(len, MAX_POPUP_ROWS.min(len));
            }
            FocusPanel::Targets => {
                let len = self.targets.len();
                self.target_state.move_down_wrap(len);
                self.target_state
                    .ensure_visible(len, MAX_POPUP_ROWS.min(len));
            }
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPanel::Callers => FocusPanel::Targets,
            FocusPanel::Targets => FocusPanel::Callers,
        };
    }

    fn clear_override_for_selected_caller(&mut self) {
        let Some(caller) = self.selected_caller_id().map(ToOwned::to_owned) else {
            return;
        };
        self.overrides.remove(&caller);
    }

    fn set_allow_all_for_selected_caller(&mut self) {
        let Some(caller) = self.selected_caller_id().map(ToOwned::to_owned) else {
            return;
        };
        if matches!(self.default_permission(&caller), EffectivePermission::All) {
            self.overrides.remove(&caller);
        } else {
            self.overrides.insert(caller, OverridePermission::All);
        }
    }

    fn toggle_selected_target(&mut self) {
        let Some(caller) = self.selected_caller_id().map(ToOwned::to_owned) else {
            return;
        };
        let Some(target_idx) = self.target_state.selected_idx else {
            return;
        };
        let Some(target) = self.targets.get(target_idx).map(|role| role.id.clone()) else {
            return;
        };

        let (current_permission, _) = self.effective_permission(&caller);
        let mut allow_list: BTreeSet<String> = match current_permission {
            EffectivePermission::All => self.all_targets_set.clone(),
            EffectivePermission::AllowList(allowed) => allowed,
            EffectivePermission::None => BTreeSet::new(),
        };
        if allow_list.contains(&target) {
            allow_list.remove(&target);
        } else {
            allow_list.insert(target);
        }

        self.set_override_allowlist_normalized(&caller, allow_list);
    }

    fn set_override_allowlist_normalized(&mut self, caller: &str, allow_list: BTreeSet<String>) {
        match self.default_permission(caller) {
            EffectivePermission::All => {
                if allow_list == self.all_targets_set {
                    self.overrides.remove(caller);
                } else {
                    self.overrides.insert(
                        caller.to_string(),
                        OverridePermission::AllowList(allow_list),
                    );
                }
            }
            EffectivePermission::AllowList(default_allowed) => {
                if allow_list == default_allowed {
                    self.overrides.remove(caller);
                } else {
                    self.overrides.insert(
                        caller.to_string(),
                        OverridePermission::AllowList(allow_list),
                    );
                }
            }
            EffectivePermission::None => {
                if allow_list.is_empty() {
                    self.overrides.remove(caller);
                } else {
                    self.overrides.insert(
                        caller.to_string(),
                        OverridePermission::AllowList(allow_list),
                    );
                }
            }
        }
    }

    fn is_dirty(&self) -> bool {
        self.overrides != self.initial_overrides
    }

    fn build_allow_overrides(&self) -> HashMap<String, Vec<String>> {
        self.overrides
            .iter()
            .map(|(caller, permission)| match permission {
                OverridePermission::All => (caller.clone(), vec!["*".to_string()]),
                OverridePermission::AllowList(allowed) => {
                    (caller.clone(), allowed.iter().cloned().collect())
                }
            })
            .collect()
    }
}

impl BottomPaneView for AgentSpawnConfigView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Up, ..
            }
            | KeyEvent {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('\u{0010}'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_up(),
            KeyEvent {
                code: KeyCode::Char('k'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_up(),
            KeyEvent {
                code: KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('n'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('\u{000e}'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_down(),
            KeyEvent {
                code: KeyCode::Char('j'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_down(),
            KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                ..
            }
            | KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::NONE,
                ..
            }
            | KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
                ..
            } => self.toggle_focus(),
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char(' '),
                modifiers: KeyModifiers::NONE,
                ..
            } => match self.focus {
                FocusPanel::Callers => self.focus = FocusPanel::Targets,
                FocusPanel::Targets => self.toggle_selected_target(),
            },
            KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.set_allow_all_for_selected_caller(),
            KeyEvent {
                code: KeyCode::Char('d'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.clear_override_for_selected_caller(),
            KeyEvent {
                code: KeyCode::Char('s'),
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                if self.is_dirty() {
                    self.app_event_tx.send(AppEvent::UpdateAgentSpawnAllow {
                        allow: self.build_allow_overrides(),
                    });
                }
                self.complete = true;
            }
            KeyEvent {
                code: KeyCode::Char('q'),
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                self.complete = true;
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }
}

impl Renderable for AgentSpawnConfigView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let content_area = render_menu_surface(area, buf);
        let header_height = self.header.desired_height(content_area.width);

        let [header_area, _, body_area, footer_area] = Layout::vertical([
            Constraint::Max(header_height),
            Constraint::Max(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(content_area);

        self.header.render(header_area, buf);

        let [callers_panel, targets_panel] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(body_area);

        self.render_callers_panel(callers_panel, buf);
        self.render_targets_panel(targets_panel, buf);

        self.footer_hint.clone().dim().render(footer_area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let width = width.max(1);
        let content_width = width.saturating_sub(4);

        let callers_width = content_width.saturating_mul(40) / 100;
        let targets_width = content_width.saturating_sub(callers_width);

        let caller_rows = self.build_caller_rows();
        let target_rows = self.build_target_rows();
        let callers_rows_height = measure_rows_height(
            &caller_rows,
            &self.caller_state,
            MAX_POPUP_ROWS,
            callers_width.saturating_add(1),
        );
        let targets_rows_height = measure_rows_height(
            &target_rows,
            &self.target_state,
            MAX_POPUP_ROWS,
            targets_width.saturating_add(1),
        );

        let list_height = callers_rows_height
            .max(targets_rows_height)
            .saturating_add(1);
        let header_height = self.header.desired_height(content_width);

        menu_surface_padding_height()
            .saturating_add(header_height)
            .saturating_add(1)
            .saturating_add(list_height)
            .saturating_add(1)
    }
}

impl AgentSpawnConfigView {
    fn render_callers_panel(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let area = area.inset(crate::render::Insets::vh(0, 1));
        let [title_area, list_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);

        let title = match self.focus {
            FocusPanel::Callers => Line::from("Callers".bold().cyan()),
            FocusPanel::Targets => Line::from("Callers".bold()),
        };
        title.render(title_area, buf);

        let rows = self.build_caller_rows();
        render_rows(
            list_area,
            buf,
            &rows,
            &self.caller_state,
            MAX_POPUP_ROWS,
            "  No caller roles available",
        );
    }

    fn render_targets_panel(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let area = area.inset(crate::render::Insets::vh(0, 1));
        let [title_area, list_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);

        let title = if let Some(caller) = self.selected_caller_id() {
            let (permission, origin) = self.effective_permission(caller);
            let permission_desc = Self::describe_permission(&permission);
            let text = format!("Targets · {caller} · {origin} · {permission_desc}");
            match self.focus {
                FocusPanel::Targets => Line::from(text.bold().cyan()),
                FocusPanel::Callers => Line::from(text.bold()),
            }
        } else {
            match self.focus {
                FocusPanel::Targets => Line::from("Targets".bold().cyan()),
                FocusPanel::Callers => Line::from("Targets".bold()),
            }
        };
        title.render(title_area, buf);

        let rows = self.build_target_rows();
        render_rows(
            list_area,
            buf,
            &rows,
            &self.target_state,
            MAX_POPUP_ROWS,
            "  Select a caller to edit",
        );
    }
}

fn normalize_role_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().replace('_', "-")
}

fn matches_allow_all_token(raw: &str) -> bool {
    let token = raw.trim();
    token == "*" || token.eq_ignore_ascii_case("all")
}

fn agent_spawn_popup_hint_line() -> Line<'static> {
    Line::from(vec![
        "Tab ".into(),
        key_hint::plain(KeyCode::Tab).into(),
        " · Toggle ".into(),
        key_hint::plain(KeyCode::Char(' ')).into(),
        " · Allow all ".into(),
        key_hint::plain(KeyCode::Char('a')).into(),
        " · Use default ".into(),
        key_hint::plain(KeyCode::Char('d')).into(),
        " · Save ".into(),
        key_hint::plain(KeyCode::Char('s')).into(),
        " · Cancel ".into(),
        key_hint::plain(KeyCode::Esc).into(),
    ])
}

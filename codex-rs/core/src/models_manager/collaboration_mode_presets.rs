use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::TUI_VISIBLE_COLLABORATION_MODES;
use codex_protocol::openai_models::ReasoningEffort;

const COLLABORATION_MODE_PLAN: &str = include_str!("../../templates/collaboration_mode/plan.md");
const COLLABORATION_MODE_DEFAULT: &str =
    include_str!("../../templates/collaboration_mode/default.md");
const COLLABORATION_MODE_HEAVY: &str = include_str!("../../templates/collaboration_mode/heavy.md");
const COLLABORATION_MODE_HEAVY_PLAN: &str =
    include_str!("../../templates/collaboration_mode/heavy_plan.md");
const KNOWN_MODE_NAMES_PLACEHOLDER: &str = "{{KNOWN_MODE_NAMES}}";
const REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER: &str = "{{REQUEST_USER_INPUT_AVAILABILITY}}";

pub(crate) fn builtin_collaboration_mode_presets() -> Vec<CollaborationModeMask> {
    vec![
        default_preset(),
        plan_preset(),
        heavy_preset(),
        heavy_plan_preset(),
    ]
}

fn plan_preset() -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::Plan.display_name().to_string(),
        mode: Some(ModeKind::Plan),
        model: None,
        reasoning_effort: Some(Some(ReasoningEffort::Medium)),
        developer_instructions: Some(Some(COLLABORATION_MODE_PLAN.to_string())),
    }
}

fn default_preset() -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::Default.display_name().to_string(),
        mode: Some(ModeKind::Default),
        model: None,
        reasoning_effort: None,
        developer_instructions: Some(Some(mode_instructions(
            COLLABORATION_MODE_DEFAULT,
            ModeKind::Default,
        ))),
    }
}

fn heavy_preset() -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::Heavy.display_name().to_string(),
        mode: Some(ModeKind::Heavy),
        model: None,
        reasoning_effort: None,
        developer_instructions: Some(Some(mode_instructions(
            COLLABORATION_MODE_HEAVY,
            ModeKind::Heavy,
        ))),
    }
}

fn heavy_plan_preset() -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::HeavyPlan.display_name().to_string(),
        mode: Some(ModeKind::HeavyPlan),
        model: None,
        reasoning_effort: Some(Some(ReasoningEffort::Medium)),
        developer_instructions: Some(Some(COLLABORATION_MODE_HEAVY_PLAN.to_string())),
    }
}

fn mode_instructions(template: &str, mode: ModeKind) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn preset_names_use_mode_display_names() {
        assert_eq!(plan_preset().name, ModeKind::Plan.display_name());
        assert_eq!(default_preset().name, ModeKind::Default.display_name());
        assert_eq!(heavy_preset().name, ModeKind::Heavy.display_name());
        assert_eq!(heavy_plan_preset().name, ModeKind::HeavyPlan.display_name());
    }

    #[test]
    fn mode_instructions_replace_mode_names_placeholder() {
        let default_instructions = default_preset()
            .developer_instructions
            .expect("default preset should include instructions")
            .expect("default instructions should be set");

        assert!(!default_instructions.contains(KNOWN_MODE_NAMES_PLACEHOLDER));
        assert!(!default_instructions.contains(REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER));

        let known_mode_names = format_mode_names(&TUI_VISIBLE_COLLABORATION_MODES);
        let expected_snippet = format!("Known mode names are {known_mode_names}.");
        assert!(default_instructions.contains(&expected_snippet));

        let expected_availability_message =
            request_user_input_availability_message(ModeKind::Default);
        assert!(default_instructions.contains(&expected_availability_message));

        let heavy_instructions = heavy_preset()
            .developer_instructions
            .expect("heavy preset should include instructions")
            .expect("heavy instructions should be set");
        let heavy_availability_message = request_user_input_availability_message(ModeKind::Heavy);
        assert!(heavy_instructions.contains(&heavy_availability_message));
    }

    #[test]
    fn builtin_presets_include_all_visible_modes_in_expected_order() {
        let builtins = builtin_collaboration_mode_presets();
        let expected_modes = vec![
            Some(ModeKind::Default),
            Some(ModeKind::Plan),
            Some(ModeKind::Heavy),
            Some(ModeKind::HeavyPlan),
        ];
        let actual_modes: Vec<Option<ModeKind>> =
            builtins.into_iter().map(|mask| mask.mode).collect();

        assert_eq!(expected_modes, actual_modes);
    }
}

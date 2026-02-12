# Collab Prompts Plugin

This plugin overrides built-in collaboration mode developer instructions with
filesystem-loaded prompt templates.

## What it changes

- Heavy mode: `collaboration_modes/heavy.md`
- Heavy Plan mode: `collaboration_modes/heavy_plan.md`

## How to enable

Add to your `config.toml`:

```toml
[plugins]
enabled = ["collab-prompts"]
```

Then restart Codex.

## Where Codex looks for plugins

Codex resolves plugin id `collab-prompts` from:

1. `$CODEX_HOME/plugins/collab-prompts` (for example `~/.coder/plugins/collab-prompts`)
2. `<current-working-directory>/plugins/collab-prompts`

If you launch `coder` from `~`, the second path becomes `~/plugins/collab-prompts`.
To use the repo copy directly, either:

- run `coder` from the repo root, or
- copy/symlink this plugin directory into `$CODEX_HOME/plugins`.

## How to edit the prompts

- Heavy: `plugins/collab-prompts/collaboration_modes/heavy.md`
- Heavy Plan: `plugins/collab-prompts/collaboration_modes/heavy_plan.md`

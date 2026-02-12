# Slash commands

For the general built-in command overview, see:

- https://developers.openai.com/codex/cli/slash-commands

## Additional local commands

This codebase currently includes these extra commands in the TUI:

- `/context`
  - Prints the most recently prepared Responses request payload snapshot.
  - Useful for debugging what was actually sent to the model provider.

- `/role-models`
  - Manages role-series model overrides for plugin roles (`fast`, `normal`, `heavy`).
  - Run `/role-models` (no args) to open an interactive picker that shows current series mappings.
  - Inline usage:
    - `/role-models show`
    - `/role-models set <fast|normal|heavy> <model>`
    - `/role-models clear <fast|normal|heavy>`
    - `/role-models reset`

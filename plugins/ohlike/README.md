# OhLike Plugin (coder-0.97)

这个目录是把你 0.94 的「多 Agent 魔改」抽象成 **Codex 插件**后的落地版本（基于 upstream `rust-v0.97.0`）。

目标：
- 把角色/多 Agent 的“业务改动”尽量收敛到 `plugins/`，升级 upstream 时只需要 rebase 一层通用的 plugin host patch + 复制插件即可；
- 提供三档角色变体 `fast/normal/heavy`，让主 agent 切换时不会污染子 agent 的模型/思考强度配置；
- 提供 `coder` 二进制（魔改入口）；`codex` 保持官方 npm 入口语义。

---

## 插件里包含什么

### 1) 角色与 Prompt（全在插件里）
- 角色都在 `roles/<agent>/{fast,normal,heavy}.md`，每个文件都是完整 prompt。
- 角色定义在 `plugin.toml` 的 `[roles.*]` 下。

### 2) 三档变体（fast/normal/heavy）

**role id 规则**：`<name>-fast | <name>-normal | <name>-heavy`

**模型策略（写死在插件里）**
- `*-fast`: `model = "gpt-5.3-codex"`, `reasoning_effort = "xhigh"`
- `*-normal`: `model = "gpt-5.2"`, `reasoning_effort = "xhigh"`
- `*-heavy`: `model = "gpt-5.2"`, `reasoning_effort = "xhigh"`

**子 agent（spawnable）策略**
- 你要求“其他 agent 默认 xhigh 且不被主 agent 切换影响”，所以所有 spawnable 角色都显式写 `reasoning_effort = "xhigh"`。

### 2.1) Shepherd 赛马编排（pass@k）

新增角色：`shepherd-fast | shepherd-normal | shepherd-heavy`

- `shepherd-*` 是只读编排者（`read_only = true`），负责：赛马分发、验收、选优。
- `shepherd-*` 只允许委派 `atlas-*`（同变体），不直接委派其它角色。
- `atlas-*` 设为 `spawnable`，由 atlas 自主组织后续执行链路。

**公平性规则（核心）**
- 同一轮候选必须使用**完全一致**的任务 prompt（canonical prompt）。
- 候选之间允许不同的只有：`candidate_id / resource_assignment / branch(worktree) / run_id`。
- 除资源与候选元数据外，任何提示词差异都视为破坏 pass@k 公平性。

### 2.2) Ruler 证据裁决（explore gate）

新增角色：`ruler-fast | ruler-normal | ruler-heavy`

- `ruler-*` 是只读裁决者（`read_only = true`），职责是对多子 agent 的结论做**证据充分性裁决**。
- `ruler-*` 不产出新事实、不写代码，只对调用方给出的 claim/evidence 做 `PASS | WARN | FAIL` 判定。
- `atlas-heavy` 默认采用强门禁链路：`explore/librarian -> ruler -> parent synthesis`。
- `atlas-fast/normal` 可选启用同一链路，推荐在高歧义探索任务中启用。

### 3) spawn 默认权限（插件内默认）
`plugin.toml` 里 `[spawn_defaults.allow]` 定义默认 allowlist：
- `shepherd-fast/normal/heavy` 仅允许 spawn `atlas-fast/normal/heavy`。
- 例如 `atlas-fast` 只允许 spawn `*-fast` 系列（具体是列举的 allowlist，避免 `*` 太宽）。
- `atlas-*` / `sisyphus-*` 默认允许 spawn 对应档位的 `ruler-*` 作为裁决门禁。
- `sisyphus-junior-fast/normal/heavy` 仅允许 spawn 对应变体的 `explore/librarian/multimodal-looker`。

> 用户侧还可以通过 `$CODEX_HOME/config.toml` 的 `[agent_spawn.allow]` 覆盖（优先级更高）。

---

## Host（Codex 核心）为了插件化做了哪些“通用改动”

> 这些不属于 OhLike 业务逻辑，但它们是让插件能工作、可升级的“最小侵入 host 层”。

### Patch 面（改了哪些地方）
core / tui 的主要改动点（用于你未来升版本时快速 grep / rebase）：
- core（插件与配置）
  - `codex-rs/core/src/plugins/mod.rs`：插件 registry + `plugin.toml` loader
  - `codex-rs/core/src/config/mod.rs`：新增 config 字段 + spawn 时应用 role policy
  - `codex-rs/core/src/config/edit.rs`：支持持久化 `[agent_spawn.allow]`
- core（多 agent 行为）
  - `codex-rs/core/src/thread_manager.rs`：spawn thread 时注入 plugin role prompt/policy
  - `codex-rs/core/src/tools/spec.rs`：`spawn_agent` tool schema（role/category）
  - `codex-rs/core/src/tools/handlers/collab.rs`：spawn_agent 解析 + 权限校验 + 变体补全
- tui（交互）
  - `codex-rs/tui/src/app.rs`：切换 role 时提示 model/effort
  - `codex-rs/tui/src/slash_command.rs` / `codex-rs/tui/src/chatwidget.rs`：`/agent-config` 入口
  - `codex-rs/tui/src/slash_command.rs` / `codex-rs/tui/src/chatwidget.rs`：`/context` 入口（打印最近一次 prepared Responses 请求上下文）
  - `codex-rs/tui/src/slash_command.rs` / `codex-rs/tui/src/chatwidget.rs`：`/role-models` 入口（按 fast/normal/heavy 系列覆盖 model 并持久化）
  - `codex-rs/tui/src/bottom_pane/agent_spawn_config_view.rs`：agent spawn 权限配置 UI
- cli（命令）
  - `codex-rs/cli/src/bin/coder.rs` + `codex-rs/cli/Cargo.toml`：新增 `coder` binary

### 1) 插件注册表与 manifest loader
- 代码：`codex-rs/core/src/plugins/mod.rs`
- 功能：从 `$CODEX_HOME/plugins/<id>/plugin.toml` 或 `<repo-root>/plugins/<id>/plugin.toml` 加载插件并合并 registry。

### 2) Config 接入 + spawn 时应用 role policy
- 代码：`codex-rs/core/src/config/mod.rs`
- 功能：
  - `ConfigToml` 增加 `[plugins].enabled` / `agent_role` / `[agent_spawn].allow`
  - spawn 线程时加载 role prompt/policy（model、reasoning_effort、read_only、collab feature 等）

### 3) spawn_agent() 变体感知解析（关键）
- 代码：`codex-rs/core/src/tools/handlers/collab.rs`
- 功能：当调用 `spawn_agent(subagent_type="explore")` 时，如果 caller 是 `atlas-fast`，会自动解析到 `explore-fast`（同理 category 默认到 `sisyphus-junior-fast/normal/heavy`）。

规则摘要：
1. 显式给了 `agent_role/subagent_type` 且存在就用；
2. 显式给了不带后缀的 role（如 `explore`），会按 caller 变体补后缀，优先 `explore-fast/normal/heavy`；
3. 只给了 `category` 时默认 `sisyphus-junior-{caller-variant}`。

### 4) TUI：切换 agent role 时提示 model/思考强度
- 代码：`codex-rs/tui/src/app.rs`
- 功能：在 Agents picker 切换成功后弹一条 info message：显示 `Model: ... • reasoning: ...`，用于明确提示“切换到了 codex 模型/当前档位”。

---

## 如何使用

1) 安装插件到 `$CODEX_HOME/plugins/ohlike`（`coder` 默认 `$CODEX_HOME=~/.coder`）
2) `~/.coder/config.toml` 启用插件并设置 role，例如：

```toml
[plugins]
enabled = ["ohlike"]

agent_role = "atlas-normal"
```

### 2.1) 覆盖整套 fast/normal/heavy 的模型

可以在配置里按系列覆盖模型（优先级高于 `plugin.toml` 里 role 默认 `model`）：

```toml
[agent_role_models.series]
fast = "gpt-5.3-codex"
normal = "gpt-5.3-codex"
heavy = "gpt-5.3-codex"
```

也可以在 TUI 里动态修改并持久化到 `config.toml`：

- `/role-models`（无参数，打开交互式面板并显示 fast/normal/heavy 当前映射）
- `/role-models show`
- `/role-models set heavy gpt-5.3-codex`
- `/role-models clear heavy`
- `/role-models reset`

3) 运行：`coder`

> 说明：`codex` 应作为官方 npm 入口使用，不建议加载该魔改插件配置。

若使用赛马编排，建议：

```toml
[plugins]
enabled = ["ohlike"]

agent_role = "shepherd-normal"
```

---

## 升级 upstream（给未来版本的“快升级”手册）

目标：下次 upstream 出 `rust-v0.98.x` 时，尽量只做两件事：
1) rebase 那层 **通用 host patch**（plugins loader / spawn hook / tui picker / spawn_agent 解析）
2) 直接复制 `plugins/ohlike`（或保持不动）即可

建议流程：
1. `git fetch upstream --tags`
2. 从新 tag 建新分支（例如 `rust-v0.98.0`）
3. 把 host patch 这几处按冲突 rebase：
   - `codex-rs/core/src/plugins/mod.rs`
   - `codex-rs/core/src/config/mod.rs`
   - `codex-rs/core/src/thread_manager.rs`
   - `codex-rs/core/src/tools/handlers/collab.rs`
   - `codex-rs/core/src/tools/spec.rs`
   - `codex-rs/tui/src/*`（Agents picker、/agent-config）
4. 跑 `just fmt` / `cargo test -p codex-core --lib` / `cargo test -p codex-tui`

---

## TODO（你后续想增强时的入口）
- heavy prompt 现在与 normal 一样（仅 reasoning_effort 不同），后面可把 `roles/*/heavy.md` 改成更“重”的验证/测试/复盘指令。
- categories 目前没有分 `fast/normal/heavy` 三套；如果要“category 也三档”，可以在 `spawn_agent` 的 category append 处按 caller variant 选择不同 append 文件。

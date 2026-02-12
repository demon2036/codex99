# Oh My OpenCode agent prompt export

Generated: 2026-02-01T06:54:04.641Z

Source repo: /home/john/github/oh-my-opencode

Notes:
- This export is intended for review ("what does oh-my inject").
- Some prompts are dynamically generated; this script renders Atlas/Sisyphus with a representative set of tools/categories/skills.
- Prometheus is exported from src/agents/prometheus-prompt.ts constant.

## Files

- roles/sisyphus/{fast,normal,heavy}.md (src/agents/sisyphus.ts:createSisyphusAgent)
- roles/atlas/{fast,normal,heavy}.md (src/agents/atlas.ts:createAtlasAgent)
- roles/sisyphus-junior/{fast,normal,heavy}.md (src/agents/sisyphus-junior.ts:createSisyphusJuniorAgentWithOverrides defaults)
- roles/prometheus/{fast,normal,heavy}.md (src/agents/prometheus-prompt.ts:PROMETHEUS_SYSTEM_PROMPT)
- roles/metis/{fast,normal,heavy}.md (src/agents/metis.ts:createMetisAgent)
- roles/momus/{fast,normal,heavy}.md (src/agents/momus.ts:createMomusAgent)
- roles/oracle/{fast,normal,heavy}.md (src/agents/oracle.ts:createOracleAgent)
- roles/ruler/{fast,normal,heavy}.md (local plugin role: evidence adjudication gate for exploration outputs)
- roles/librarian/{fast,normal,heavy}.md (src/agents/librarian.ts:createLibrarianAgent)
- roles/explore/{fast,normal,heavy}.md (src/agents/explore.ts:createExploreAgent)
- roles/multimodal-looker/{fast,normal,heavy}.md (src/agents/multimodal-looker.ts:createMultimodalLookerAgent)
- roles/shepherd/{fast,normal,heavy}.md (local plugin role: pass@k race orchestration, delegates only to atlas)

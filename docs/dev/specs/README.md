# Migration specs

Behavioural specifications of React areas, extracted from the TSX during the
Dioxus migration so each area can be rebuilt in Rust without re-reading the
original components. They describe the code as of v3.17.0; the React sources
remain authoritative until Phase 6, after which this directory is deleted.

- `config-utils.md`: providerConfigUtils / tomlUtils / grokBuildConfig / version (ported to `crates/cc-switch-config`)
- `settings-area.md`: Settings page, tabs, save flow, About, usage tab
- `providers-list.md`: provider list, cards, actions, header toggles, add/edit entry points

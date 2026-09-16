// Providers area fixtures and handlers.
(() => {
  const { state, register } = window.__TAURI_MOCK__;
  state.providers = {
    claude: {
      "p-official": { id: "p-official", name: "Anthropic Official", settingsConfig: { env: {} }, websiteUrl: "https://anthropic.com", icon: "anthropic", sortIndex: 1, category: "official" },
      "p-kimi": { id: "p-kimi", name: "Kimi For Coding", settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://api.kimi.com/coding" } }, websiteUrl: "https://kimi.com", icon: "kimi", sortIndex: 0, notes: "team account", category: "third_party", meta: { isPartner: true } },
    },
    codex: {},
    gemini: {},
  };
  state.current = { claude: "p-official", codex: "", gemini: "" };
  register({
    get_providers: ({ app }) => state.providers[app] ?? {},
    get_current_provider: ({ app }) => state.current[app] ?? "",
    switch_provider: ({ app, id }) => { state.current[app] = id; return { warnings: [] }; },
    delete_provider: ({ app, id }) => { delete state.providers[app][id]; return true; },
    add_provider: ({ app, provider }) => { const id = provider.id || `p-${Date.now()}`; state.providers[app] ??= {}; state.providers[app][id] = { ...provider, id }; return true; },
    update_provider: ({ app, provider }) => { state.providers[app][provider.id] = provider; return true; },
    update_providers_sort_order: ({ app, updates }) => { for (const u of updates) if (state.providers[app]?.[u.id]) state.providers[app][u.id].sortIndex = u.sortIndex; return true; },
  });
})();

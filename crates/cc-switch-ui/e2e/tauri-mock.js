// Minimal stand-in for the Tauri runtime injected into the webview, so the
// Dioxus bundle can be exercised in a plain headless browser. Extend the
// `handlers` table as views are ported.
(() => {
  const listeners = new Map();
  const state = {
    settings: {
      showInTray: true,
      minimizeToTrayOnClose: false,
      useAppWindowControls: false,
      enableClaudePluginIntegration: false,
      skipClaudeOnboarding: false,
      launchOnStartup: false,
      silentStartup: false,
      enableLocalProxy: true,
      enableFailoverToggle: false,
      showProfileSwitcher: true,
      preserveCodexOfficialAuthOnSwitch: false,
      unifyCodexSessionHistory: false,
      language: "en",
      visibleApps: { claude: true, "claude-desktop": false, codex: true, gemini: true, grokbuild: false, opencode: false, openclaw: false, hermes: false },
      webdavSync: { enabled: false },
    },
  };
  state.providers = {
    claude: {
      "p-official": { id: "p-official", name: "Anthropic Official", settingsConfig: { env: {} }, websiteUrl: "https://anthropic.com", icon: "anthropic", sortIndex: 1, category: "official" },
      "p-kimi": { id: "p-kimi", name: "Kimi For Coding", settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://api.kimi.com/coding" } }, websiteUrl: "https://kimi.com", icon: "kimi", sortIndex: 0, notes: "team account", meta: { isPartner: true } },
    },
    codex: {},
  };
  state.current = { claude: "p-official", codex: "" };
  const handlers = {
    get_settings: () => state.settings,
    get_providers: ({ app }) => state.providers[app] ?? {},
    get_current_provider: ({ app }) => state.current[app] ?? "",
    switch_provider: ({ app, id }) => { state.current[app] = id; return { warnings: [] }; },
    delete_provider: ({ app, id }) => { delete state.providers[app][id]; return true; },
    open_external: () => null,
    save_settings: ({ settings }) => { state.settings = settings; return true; },
    set_window_theme: () => null,
    get_init_error: () => null,
  };
  window.__TAURI_MOCK__ = { state, handlers, calls: [] };
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        window.__TAURI_MOCK__.calls.push({ cmd, args });
        const handler = handlers[cmd];
        if (!handler) throw `mock: unknown command ${cmd}`;
        return handler(args ?? {});
      },
    },
    event: {
      listen: async (name, cb) => {
        if (!listeners.has(name)) listeners.set(name, new Set());
        listeners.get(name).add(cb);
        return () => listeners.get(name).delete(cb);
      },
      emit: async (name, payload) => {
        for (const cb of listeners.get(name) ?? []) cb({ event: name, payload });
      },
    },
  };
})();

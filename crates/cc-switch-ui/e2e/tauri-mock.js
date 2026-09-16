// Minimal stand-in for the Tauri runtime injected into the webview, so the
// Dioxus bundle can be exercised in a plain headless browser.
//
// Handlers are registered per area from e2e/handlers/*.js via
// `window.__TAURI_MOCK__.register({ command: (args) => result })`; `state`
// is shared mutable fixture data.
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
  const handlers = {
    get_settings: () => state.settings,
    save_settings: ({ settings }) => { state.settings = settings; return true; },
    set_window_theme: () => null,
    get_init_error: () => null,
    open_external: () => null,
    update_tray_menu: () => null,
  };
  const mock = {
    state,
    handlers,
    calls: [],
    register: (more) => Object.assign(handlers, more),
    emit: async (name, payload) => { for (const cb of listeners.get(name) ?? []) cb({ event: name, payload }); },
  };
  window.__TAURI_MOCK__ = mock;
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        mock.calls.push({ cmd, args });
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
      emit: mock.emit,
    },
  };
})();

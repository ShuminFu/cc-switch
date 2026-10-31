/**
 * Manual install command snippets for the About tab (FE-UI-19, FE-UI-45).
 *
 * Several snippets download and run a remote install script. The snippet
 * text and `INSTALL_SCRIPT_SOURCES` make the exact URLs visible, and the
 * Windows Hermes command (a base64 `-EncodedCommand`) is shown decoded next
 * to it, so users can review what will run before copying it.
 */

export const posixScriptInstallCommand = (url: string) =>
  `bash -c 'tmp=$(mktemp) && curl -fsSL ${url} -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'`;

export const HERMES_WINDOWS_INSTALL_SCRIPT =
  "irm https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1 | iex";

/** UTF-16LE base64, the format PowerShell's `-EncodedCommand` expects. */
export const powershellEncodedCommand = (script: string): string => {
  let binary = "";
  for (let i = 0; i < script.length; i += 1) {
    const code = script.charCodeAt(i);
    binary += String.fromCharCode(code & 0xff, code >> 8);
  }
  return btoa(binary);
};

/** Inverse of {@link powershellEncodedCommand}; `null` for invalid input. */
export const decodePowershellEncodedCommand = (
  encoded: string,
): string | null => {
  try {
    const binary = atob(encoded);
    if (binary.length % 2 !== 0) return null;
    let script = "";
    for (let i = 0; i < binary.length; i += 2) {
      script += String.fromCharCode(
        binary.charCodeAt(i) | (binary.charCodeAt(i + 1) << 8),
      );
    }
    return script;
  } catch {
    return null;
  }
};

export const HERMES_WINDOWS_INSTALL_COMMAND = `powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand ${powershellEncodedCommand(
  HERMES_WINDOWS_INSTALL_SCRIPT,
)}`;

/** Remote install scripts referenced by the snippets, per platform. */
export const INSTALL_SCRIPT_SOURCES = {
  posix: [
    { tool: "Claude Code", url: "https://claude.ai/install.sh" },
    { tool: "OpenCode", url: "https://opencode.ai/install" },
    {
      tool: "Hermes",
      url: "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh",
    },
  ],
  windows: [
    {
      tool: "Hermes",
      url: "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1",
    },
  ],
} as const;

export const POSIX_ONE_CLICK_INSTALL_COMMANDS = `# Claude Code
${posixScriptInstallCommand("https://claude.ai/install.sh")} || npm i -g @anthropic-ai/claude-code@latest
# Codex
npm i -g @openai/codex@latest
# Gemini CLI
npm i -g @google/gemini-cli@latest
# Grok Build
npm i -g @xai-official/grok@latest
# OpenCode
${posixScriptInstallCommand("https://opencode.ai/install")} || npm i -g opencode-ai@latest
# OpenClaw
npm i -g openclaw@latest
# Hermes
${posixScriptInstallCommand("https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh")}`;

export const WINDOWS_ONE_CLICK_INSTALL_COMMANDS = `# Claude Code
npm i -g @anthropic-ai/claude-code@latest
# Codex
npm i -g @openai/codex@latest
# Gemini CLI
npm i -g @google/gemini-cli@latest
# Grok Build
npm i -g @xai-official/grok@latest
# OpenCode
npm i -g opencode-ai@latest
# OpenClaw
npm i -g openclaw@latest
# Hermes (runs: ${HERMES_WINDOWS_INSTALL_SCRIPT})
${HERMES_WINDOWS_INSTALL_COMMAND}`;

export function oneClickInstallCommands(windows: boolean): string {
  return windows
    ? WINDOWS_ONE_CLICK_INSTALL_COMMANDS
    : POSIX_ONE_CLICK_INSTALL_COMMANDS;
}

export function installScriptSources(windows: boolean) {
  return windows
    ? INSTALL_SCRIPT_SOURCES.windows
    : INSTALL_SCRIPT_SOURCES.posix;
}

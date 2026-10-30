import { parse as parseToml, stringify as stringifyToml } from "smol-toml";

export const GROK_BUILD_DEFAULT_MODEL = "grok-4.5";
export const GROK_BUILD_DEFAULT_API_BACKEND = "responses";
export const GROK_BUILD_DEFAULT_CONTEXT_WINDOW = 500000;

export interface GrokBuildConfigValues {
  /** Client-visible profile selected by [models].default. */
  model: string;
  /** Real model sent to the upstream provider. */
  upstreamModel?: string;
  baseUrl: string;
  name: string;
  apiKey: string;
  envKey?: string;
  apiBackend: string;
  contextWindow: number;
}

const asRecord = (value: unknown): Record<string, unknown> | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;

const asString = (value: unknown, fallback = "") =>
  typeof value === "string" ? value : fallback;

export function parseGrokBuildConfig(
  configToml: string | undefined,
  fallbackName = "",
): GrokBuildConfigValues {
  const fallback: GrokBuildConfigValues = {
    model: GROK_BUILD_DEFAULT_MODEL,
    upstreamModel: GROK_BUILD_DEFAULT_MODEL,
    baseUrl: "",
    name: fallbackName,
    apiKey: "",
    apiBackend: GROK_BUILD_DEFAULT_API_BACKEND,
    contextWindow: GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
  };

  if (!configToml?.trim()) return fallback;

  try {
    const root = asRecord(parseToml(configToml));
    const models = asRecord(root?.models);
    const defaultModel = asString(models?.default, GROK_BUILD_DEFAULT_MODEL);
    const modelTables = asRecord(root?.model);
    const selectedModel = asRecord(modelTables?.[defaultModel]);
    const rawContextWindow = selectedModel?.context_window;

    return {
      model: defaultModel,
      upstreamModel: asString(selectedModel?.model, defaultModel),
      baseUrl: asString(selectedModel?.base_url),
      name: asString(selectedModel?.name, fallbackName),
      apiKey: asString(selectedModel?.api_key),
      envKey: asString(selectedModel?.env_key),
      apiBackend: asString(
        selectedModel?.api_backend,
        GROK_BUILD_DEFAULT_API_BACKEND,
      ),
      contextWindow:
        typeof rawContextWindow === "number" &&
        Number.isInteger(rawContextWindow) &&
        rawContextWindow > 0
          ? rawContextWindow
          : GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
    };
  } catch {
    return fallback;
  }
}

export function buildGrokBuildConfig(values: GrokBuildConfigValues): string {
  return updateGrokBuildConfig(undefined, values);
}

export function updateGrokBuildConfig(
  configToml: string | undefined,
  values: GrokBuildConfigValues,
): string {
  const profile = values.model.trim() || GROK_BUILD_DEFAULT_MODEL;
  const upstreamModel = values.upstreamModel?.trim() || profile;
  let config: Record<string, unknown> = {};

  try {
    config = asRecord(configToml?.trim() ? parseToml(configToml) : {}) ?? {};
  } catch {
    config = {};
  }

  const existingModels = asRecord(config.models) ?? {};
  const previousProfile = asString(existingModels.default, profile);
  config.models = { ...existingModels, default: profile };

  const modelTables = asRecord(config.model) ?? {};
  const existingSelected =
    asRecord(modelTables[profile]) ??
    asRecord(modelTables[previousProfile]) ??
    {};
  const apiKey = values.apiKey.trim();
  const envKey =
    values.envKey?.trim() || asString(existingSelected.env_key).trim();
  const updatedSelected: Record<string, unknown> = {
    ...existingSelected,
    model: upstreamModel,
    base_url: values.baseUrl.trim(),
    name: values.name.trim(),
    api_backend: values.apiBackend.trim() || GROK_BUILD_DEFAULT_API_BACKEND,
    context_window:
      Number.isInteger(values.contextWindow) && values.contextWindow > 0
        ? values.contextWindow
        : GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
  };
  if (apiKey) updatedSelected.api_key = apiKey;
  else delete updatedSelected.api_key;
  if (envKey) updatedSelected.env_key = envKey;
  else delete updatedSelected.env_key;

  config.model = {
    ...modelTables,
    [profile]: updatedSelected,
  };

  if (previousProfile !== profile && previousProfile in modelTables) {
    delete (config.model as Record<string, unknown>)[previousProfile];
  }

  return `${stringifyToml(config).trim()}\n`;
}

const SELECTED_MODEL_FIELDS = [
  "model",
  "base_url",
  "name",
  "api_backend",
  "context_window",
  "api_key",
  "env_key",
] as const;

const tomlScalar = (value: unknown): string =>
  typeof value === "number" ? String(value) : JSON.stringify(String(value));

/** Structural equality that ignores object key order. */
const deepEqual = (a: unknown, b: unknown): boolean => {
  if (a === b) return true;
  if (Array.isArray(a) || Array.isArray(b)) {
    return (
      Array.isArray(a) &&
      Array.isArray(b) &&
      a.length === b.length &&
      a.every((item, index) => deepEqual(item, b[index]))
    );
  }
  const ra = asRecord(a);
  const rb = asRecord(b);
  if (!ra || !rb) return false;
  const keys = Object.keys(ra);
  return (
    keys.length === Object.keys(rb).length &&
    keys.every((key) => key in rb && deepEqual(ra[key], rb[key]))
  );
};

const sectionHeaderName = (line: string): string | null => {
  const match = line.trim().match(/^\[\s*([^[\]]+?)\s*\](?:\s*#.*)?$/);
  if (!match) return null;
  // Quote-aware split: [model."grok-4.5"] has a dot inside the quoted key.
  const parts: string[] = [];
  const segment = /\s*(?:"([^"]*)"|'([^']*)'|([^.\s"']+))\s*(\.|$)/y;
  let rest = match[1];
  while (rest.length > 0) {
    segment.lastIndex = 0;
    const part = segment.exec(rest);
    if (!part) return null;
    parts.push(part[1] ?? part[2] ?? part[3]);
    rest = rest.slice(part[0].length);
    if (part[4] === "") break;
  }
  return parts.join(".");
};

/**
 * Like {@link updateGrokBuildConfig}, but patches the existing lines of the
 * selected `[model.<profile>]` table in place so comments, ordering and
 * unrelated formatting in a hand-edited config.toml survive structured
 * field edits (FE-PROV-31). Falls back to the full rewrite when the profile
 * changes or the patched document would not match the full rewrite.
 */
export function updateGrokBuildConfigPreservingFormat(
  configToml: string | undefined,
  values: GrokBuildConfigValues,
): string {
  const rewritten = updateGrokBuildConfig(configToml, values);
  if (!configToml?.trim()) return rewritten;

  let current: Record<string, unknown> | undefined;
  let target: Record<string, unknown> | undefined;
  try {
    current = asRecord(parseToml(configToml));
    target = asRecord(parseToml(rewritten));
  } catch {
    return rewritten;
  }
  const profile = asString(asRecord(target?.models)?.default);
  if (asString(asRecord(current?.models)?.default) !== profile) {
    return rewritten;
  }
  const wanted = asRecord(asRecord(target?.model)?.[profile]);
  if (!wanted) return rewritten;

  const lines = configToml.replace(/\r\n/g, "\n").split("\n");
  const start = lines.findIndex(
    (line) => sectionHeaderName(line) === `model.${profile}`,
  );
  if (start === -1) return rewritten;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i += 1) {
    if (sectionHeaderName(lines[i]) !== null) {
      end = i;
      break;
    }
  }

  const body = lines.slice(start + 1, end);
  for (const field of SELECTED_MODEL_FIELDS) {
    const pattern = new RegExp(`^(\\s*)${field}\\s*=`);
    const index = body.findIndex((line) => pattern.test(line));
    const value = wanted[field];
    if (value === undefined) {
      if (index !== -1) body.splice(index, 1);
      continue;
    }
    const assignment = `${field} = ${tomlScalar(value)}`;
    if (index !== -1) {
      const indent = body[index].match(pattern)?.[1] ?? "";
      body[index] = `${indent}${assignment}`;
    } else {
      // Insert after the last non-blank line of the table.
      let insertAt = body.length;
      while (insertAt > 0 && body[insertAt - 1].trim() === "") insertAt -= 1;
      body.splice(insertAt, 0, assignment);
    }
  }

  const patched = [
    ...lines.slice(0, start + 1),
    ...body,
    ...lines.slice(end),
  ].join("\n");
  try {
    return deepEqual(parseToml(patched), target) ? patched : rewritten;
  } catch {
    return rewritten;
  }
}

export function validateGrokBuildConfig(configToml: string): string | null {
  if (!configToml.trim()) return "config.toml must not be empty";
  try {
    const root = asRecord(parseToml(configToml));
    const models = asRecord(root?.models);
    const profile = asString(models?.default).trim();
    const selected = asRecord(asRecord(root?.model)?.[profile]);
    if (!profile || !selected) return "Missing [models] default model table";
    for (const field of ["model", "base_url", "name", "api_backend"]) {
      if (!asString(selected[field]).trim()) return `Missing ${field}`;
    }
    if (
      !asString(selected.api_key).trim() &&
      !asString(selected.env_key).trim()
    ) {
      return "Missing api_key or env_key";
    }
    const contextWindow = selected.context_window;
    if (
      typeof contextWindow !== "number" ||
      !Number.isInteger(contextWindow) ||
      contextWindow <= 0
    ) {
      return "context_window must be a positive integer";
    }
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : "Invalid TOML";
  }
}

export function extractGrokBuildBaseUrl(configToml: string): string {
  return parseGrokBuildConfig(configToml).baseUrl;
}

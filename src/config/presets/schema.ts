import { z } from "zod";

/**
 * Presets as data (FE-ARCH-34, first increment).
 *
 * Built-in preset tables move from TypeScript arrays to JSON files under
 * `src/config/presets/`, validated by these zod schemas when loaded. A
 * malformed entry is reported and skipped instead of breaking the preset
 * picker. Sponsor flags (`isPartner`, `primePartner`, `partnerPromotionKey`)
 * stay explicit fields of the schema.
 */

const category = z.enum([
  "official",
  "cn_official",
  "cloud_provider",
  "aggregator",
  "third_party",
  "custom",
  "omo",
  "omo-slim",
]);

const sponsorFields = {
  isPartner: z.boolean().optional(),
  primePartner: z.boolean().optional(),
  partnerPromotionKey: z.string().optional(),
};

export const geminiPresetSchema = z
  .object({
    name: z.string().min(1),
    nameKey: z.string().optional(),
    websiteUrl: z.string(),
    apiKeyUrl: z.string().optional(),
    settingsConfig: z.object({
      env: z.record(z.string(), z.string()),
      config: z.record(z.string(), z.unknown()).optional(),
    }),
    baseURL: z.string().optional(),
    model: z.string().optional(),
    description: z.string().optional(),
    category: category.optional(),
    endpointCandidates: z.array(z.string()).optional(),
    theme: z
      .object({
        icon: z.enum(["gemini", "generic"]).optional(),
        backgroundColor: z.string().optional(),
        textColor: z.string().optional(),
      })
      .optional(),
    icon: z.string().optional(),
    iconColor: z.string().optional(),
    ...sponsorFields,
  })
  .strict();

export interface PresetValidationIssue {
  index: number;
  name?: string;
  message: string;
}

/**
 * Validate a preset table. Returns the valid entries (in order) and one issue
 * per rejected entry; a table that is not an array yields no presets.
 */
export function validatePresetTable<S extends z.ZodTypeAny>(
  schema: S,
  data: unknown,
): { presets: z.infer<S>[]; issues: PresetValidationIssue[] } {
  if (!Array.isArray(data)) {
    return {
      presets: [],
      issues: [{ index: -1, message: "preset table is not an array" }],
    };
  }
  const presets: z.infer<S>[] = [];
  const issues: PresetValidationIssue[] = [];
  data.forEach((entry, index) => {
    const result = schema.safeParse(entry);
    if (result.success) {
      presets.push(result.data);
    } else {
      const name =
        entry && typeof entry === "object" && "name" in entry
          ? String((entry as { name: unknown }).name)
          : undefined;
      issues.push({
        index,
        name,
        message: result.error.issues
          .map(
            (issue) => `${issue.path.join(".") || "(root)"}: ${issue.message}`,
          )
          .join("; "),
      });
    }
  });
  return { presets, issues };
}

/** Validate and log (never throw): the picker shows whatever is valid. */
export function loadPresetTable<S extends z.ZodTypeAny>(
  label: string,
  schema: S,
  data: unknown,
): z.infer<S>[] {
  const { presets, issues } = validatePresetTable(schema, data);
  for (const issue of issues) {
    console.warn(
      `[presets] skipped ${label} preset #${issue.index}${issue.name ? ` (${issue.name})` : ""}: ${issue.message}`,
    );
  }
  return presets;
}

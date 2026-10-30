import { describe, it, expect } from "vitest";
import {
  AUTO_FAILOVER_RANGES,
  validateAutoFailoverForm,
  type AutoFailoverNumericField,
} from "@/components/proxy/autoFailoverSchema";

const valid = (): Record<AutoFailoverNumericField, string> =>
  Object.fromEntries(
    Object.entries(AUTO_FAILOVER_RANGES).map(([field, { min }]) => [
      field,
      String(min),
    ]),
  ) as Record<AutoFailoverNumericField, string>;

describe("autoFailoverSchema (FE-UI-20)", () => {
  it("accepts every boundary value", () => {
    expect(validateAutoFailoverForm(valid()).success).toBe(true);
    const max = Object.fromEntries(
      Object.entries(AUTO_FAILOVER_RANGES).map(([f, { max }]) => [
        f,
        ` ${max} `,
      ]),
    ) as Record<AutoFailoverNumericField, string>;
    const result = validateAutoFailoverForm(max);
    expect(result.success).toBe(true);
    if (result.success) expect(result.data.circuitMinRequests).toBe(100);
  });

  it.each(Object.entries(AUTO_FAILOVER_RANGES))(
    "rejects %s just outside its range, blank and non-integers",
    (field, { min, max }) => {
      for (const bad of [String(min - 1), String(max + 1), "", "1.5", "x"]) {
        const form = { ...valid(), [field]: bad };
        const result = validateAutoFailoverForm(form);
        expect(result.success).toBe(false);
        if (!result.success) expect(result.invalidFields).toEqual([field]);
      }
    },
  );
});

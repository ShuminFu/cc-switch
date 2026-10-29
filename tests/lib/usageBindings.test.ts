import { beforeEach, describe, expect, expectTypeOf, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { commands, type ErrorCode, type IpcError } from "@/shared/bindings";
import type { IpcErrorCode, IpcErrorPayload } from "@/lib/api/ipc";
import { usageApi } from "@/lib/api/usage";

/** 依次调用每个函数，收集其触发的 invoke 命令名 */
async function collectCommandNames(
  calls: Array<() => Promise<unknown>>,
): Promise<string[]> {
  const names: string[] = [];
  for (const call of calls) {
    invokeMock.mockClear();
    await call();
    names.push(invokeMock.mock.calls[0]?.[0] as string);
  }
  return names.sort();
}

describe("generated usage bindings (CCS-116)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(null);
  });

  it("keeps the hand-written IpcError vocabulary in sync with the backend", () => {
    expectTypeOf<IpcErrorCode>().toEqualTypeOf<ErrorCode>();
    expectTypeOf<IpcError>().toMatchTypeOf<IpcErrorPayload>();
  });

  it("usageApi invokes exactly the commands exported for the usage domain", async () => {
    const generated = await collectCommandNames(
      Object.values(commands).map(
        (command) => () =>
          (command as (...args: unknown[]) => Promise<unknown>)(),
      ),
    );

    const handWritten = await collectCommandNames([
      () => usageApi.getUsageSummary(),
      () => usageApi.getUsageSummaryByApp(),
      () => usageApi.getUsageTrends(),
      () => usageApi.getProviderStats(),
      () => usageApi.getModelStats(),
      () => usageApi.getRequestLogs({}),
      () => usageApi.getRequestDetail("req"),
      () => usageApi.getModelPricing(),
      () => usageApi.updateModelPricing("m", "M", "0", "0", "0", "0"),
      () => usageApi.deleteModelPricing("m"),
      () => usageApi.checkProviderLimits("p", "claude"),
      () => usageApi.syncSessionUsage(),
      () => usageApi.getDataSourceBreakdown(),
    ]);

    expect(handWritten).toEqual(generated);
    expect(generated).toHaveLength(13);
  });
});

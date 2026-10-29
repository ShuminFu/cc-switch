import { describe, expect, expectTypeOf, it } from "vitest";
import {
  COMMANDS,
  type AppType_Serialize,
  type UsageSummary,
} from "@/shared/bindings";
import type { AppId } from "@/lib/api/types";
import type { CommandName, CommandResult } from "./commands";
import { handlers } from "./handlers";
import { UNMOCKED_COMMANDS } from "./unmockedCommands";

const TAURI_ENDPOINT = "http://tauri.local/";

const mockedCommands = handlers.map((handler) =>
  String(handler.info.path).replace(TAURI_ENDPOINT, ""),
);

const duplicates = (names: readonly string[]) =>
  names.filter((name, index) => names.indexOf(name) !== index);

describe("MSW handlers vs generated command list (CCS-117)", () => {
  it("only mocks commands that the backend registers", () => {
    const registered = new Set<string>(COMMANDS);
    expect(mockedCommands.filter((name) => !registered.has(name))).toEqual([]);
    expect(duplicates(mockedCommands)).toEqual([]);
  });

  it("covers every registered command with a handler or the allowlist", () => {
    const covered = new Set<string>([...mockedCommands, ...UNMOCKED_COMMANDS]);
    expect(COMMANDS.filter((name) => !covered.has(name))).toEqual([]);
  });

  it("keeps the allowlist free of stale or mocked entries", () => {
    const mocked = new Set(mockedCommands);
    expect(UNMOCKED_COMMANDS.filter((name) => mocked.has(name))).toEqual([]);
    expect(duplicates(UNMOCKED_COMMANDS)).toEqual([]);
  });

  it("types handler results against the generated bindings", () => {
    expectTypeOf<AppId>().toEqualTypeOf<AppType_Serialize>();
    expectTypeOf<
      CommandResult<"get_usage_summary">
    >().toEqualTypeOf<UsageSummary>();
    expectTypeOf<
      CommandResult<"get_current_provider">
    >().toEqualTypeOf<string>();
    expectTypeOf<"get_providers">().toMatchTypeOf<CommandName>();
    // @ts-expect-error 未注册的命令名不是 CommandName
    expectTypeOf<"not_a_command">().toMatchTypeOf<CommandName>();
  });
});

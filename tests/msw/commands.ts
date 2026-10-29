import type {
  COMMANDS,
  commands,
  LiveWritePreview,
  Provider_Serialize,
  ProxyStatus,
} from "@/shared/bindings";

/** `src-tauri/src/lib.rs` 中 `generate_handler!` 注册的全部命令名（生成）。 */
export type CommandName = (typeof COMMANDS)[number];

type SnakeToCamel<S extends string> = S extends `${infer Head}_${infer Tail}`
  ? `${Head}${Capitalize<SnakeToCamel<Tail>>}`
  : S;

type GeneratedCommands = typeof commands;

/**
 * 尚未带 specta 签名的命令：返回值取自生成的 DTO 类型。
 * 命令接入 `collect_commands!` 后应从这里删除。
 */
interface ResultOverrides {
  get_providers: Record<string, Provider_Serialize>;
  get_current_provider: string;
  get_proxy_status: ProxyStatus;
  preview_live_write: LiveWritePreview;
}

/** 命令的返回值类型；没有生成类型的命令为 `unknown`。 */
export type CommandResult<K extends CommandName> =
  SnakeToCamel<K> extends keyof GeneratedCommands
    ? Awaited<ReturnType<GeneratedCommands[SnakeToCamel<K>]>>
    : K extends keyof ResultOverrides
      ? ResultOverrides[K]
      : unknown;

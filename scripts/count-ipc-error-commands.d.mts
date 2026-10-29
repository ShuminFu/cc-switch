export function scanCommands(
  source: string,
): { name: string; ipcError: boolean }[];
export function countIpcErrorCommands(files: Record<string, string>): {
  total: number;
  ipcError: number;
  percent: number;
  perFile: { file: string; total: number; ipcError: number }[];
};
export function readCommandSources(root: string): Record<string, string>;

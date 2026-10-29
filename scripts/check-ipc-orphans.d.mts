export function registeredCommands(libSource: string): string[];
export function checkIpcCommands(
  libSource: string,
  frontendFiles: Record<string, string>,
): {
  total: number;
  orphans: string[];
  unknown: string[];
  badNames: string[];
};
export function readSources(root: string): {
  libSource: string;
  frontendFiles: Record<string, string>;
};

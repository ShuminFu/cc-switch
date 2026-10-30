export function registeredCommands(registrySource: string): string[];
export function checkIpcCommands(
  registrySource: string,
  frontendFiles: Record<string, string>,
): {
  total: number;
  orphans: string[];
  unknown: string[];
  badNames: string[];
};
export function readSources(root: string): {
  registrySource: string;
  frontendFiles: Record<string, string>;
};

/** Variable names whose values are credentials (FE-UI-53). */
const SECRET_NAME = /KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|AUTH/i;

export const isSecretEnvName = (name: string): boolean =>
  SECRET_NAME.test(name);

/**
 * Mask a credential for display, keeping a short recognisable prefix (e.g.
 * `sk-ant-`) and the last four characters: `sk-ant-…3456`. Short values are
 * fully masked. Non-secret variables are returned unchanged.
 */
export function maskEnvValue(name: string, value: string): string {
  if (!isSecretEnvName(name)) return value;
  if (value.length <= 12) return "•".repeat(Math.max(value.length, 8));
  const prefixMatch = /^[A-Za-z]{2,4}-(?:[A-Za-z]{2,4}-)?/.exec(value);
  const prefix = prefixMatch ? prefixMatch[0] : value.slice(0, 3);
  return `${prefix}…${value.slice(-4)}`;
}

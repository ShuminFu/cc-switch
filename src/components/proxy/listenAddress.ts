/**
 * Whether a listen address only accepts connections from this machine.
 * Anything else (0.0.0.0, ::, a LAN IP) exposes the proxy to other hosts,
 * which then must send the access token.
 */
export function isLoopbackListenAddress(address: string): boolean {
  const addr = address
    .trim()
    .toLowerCase()
    .replace(/^\[|\]$/g, "");
  if (addr === "" || addr === "localhost" || addr === "::1") return true;
  if (/^127(\.\d{1,3}){3}$/.test(addr)) return true;
  if (/^::ffff:127(\.\d{1,3}){3}$/.test(addr)) return true;
  return false;
}

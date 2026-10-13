/**
 * Whether a proxy listen address only accepts connections from this machine.
 *
 * Anything else (0.0.0.0, ::, a LAN IP) exposes the local proxy — which has no
 * client authentication and forwards requests with the user's API keys — to
 * other devices on the network.
 */
export function isLoopbackListenAddress(address: string): boolean {
  const addr = address
    .trim()
    .toLowerCase()
    .replace(/^\[|\]$/g, "");
  if (addr === "" || addr === "localhost") return true;
  if (/^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(addr)) return true;
  return addr === "::1" || addr === "0:0:0:0:0:0:0:1";
}

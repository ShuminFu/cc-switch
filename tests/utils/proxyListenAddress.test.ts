import { describe, expect, it } from "vitest";
import { isLoopbackListenAddress } from "@/components/proxy/listenAddress";

describe("isLoopbackListenAddress", () => {
  it("treats loopback addresses as local-only", () => {
    for (const addr of [
      "127.0.0.1",
      "127.1.2.3",
      "localhost",
      "::1",
      "[::1]",
      " 127.0.0.1 ",
    ]) {
      expect(isLoopbackListenAddress(addr)).toBe(true);
    }
  });

  it("flags addresses reachable from other hosts", () => {
    for (const addr of [
      "0.0.0.0",
      "::",
      "192.168.1.5",
      "10.0.0.2",
      "fe80::1",
    ]) {
      expect(isLoopbackListenAddress(addr)).toBe(false);
    }
  });
});

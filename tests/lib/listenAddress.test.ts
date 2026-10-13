import { describe, expect, it } from "vitest";
import { isLoopbackListenAddress } from "@/lib/utils/listenAddress";

describe("isLoopbackListenAddress", () => {
  it("treats loopback addresses as local", () => {
    for (const addr of [
      "127.0.0.1",
      " 127.0.0.2 ",
      "localhost",
      "::1",
      "[::1]",
      "",
    ]) {
      expect(isLoopbackListenAddress(addr)).toBe(true);
    }
  });

  it("flags addresses reachable from the network", () => {
    for (const addr of [
      "0.0.0.0",
      "::",
      "192.168.1.10",
      "10.0.0.5",
      "fe80::1",
    ]) {
      expect(isLoopbackListenAddress(addr)).toBe(false);
    }
  });
});

#!/usr/bin/env node
// Regenerate src/lib/bindings.ts from the Rust command signatures (CMD-13).
//
// The generator is the `commands::bindings` cargo test: with UPDATE_BINDINGS=1
// it writes the file, without it (as CI runs it) it fails when the checked-in
// file is stale.
import { spawnSync } from "node:child_process";

const result = spawnSync(
  "cargo",
  [
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--lib",
    "commands::bindings",
  ],
  {
    stdio: "inherit",
    env: { ...process.env, UPDATE_BINDINGS: "1" },
    shell: process.platform === "win32",
  },
);

process.exit(result.status ?? 1);

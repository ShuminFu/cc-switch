import { describe, it, expect } from "vitest";
import { parseRepoUrl } from "@/components/skills/parseRepoUrl";

describe("parseRepoUrl (FE-UI-60)", () => {
  it.each([
    ["owner/repo", { owner: "owner", name: "repo" }],
    ["https://github.com/o/r", { owner: "o", name: "r" }],
    ["https://github.com/o/r/", { owner: "o", name: "r" }],
    ["github.com/o/r", { owner: "o", name: "r" }],
    ["https://www.github.com/o/r", { owner: "o", name: "r" }],
    ["git@github.com:o/r.git", { owner: "o", name: "r" }],
    ["https://github.com/o/r.git", { owner: "o", name: "r" }],
    [
      "https://github.com/o/r/tree/dev/skills",
      { owner: "o", name: "r", branch: "dev" },
    ],
    ["  https://github.com/o/r  ", { owner: "o", name: "r" }],
  ])("parses %s", (input, expected) => {
    expect(parseRepoUrl(input)).toEqual(expected);
  });

  it.each(["", "o", "https://gitlab.com/o/r", "o/r/blob/main/x", "o w/r"])(
    "rejects %j",
    (input) => {
      expect(parseRepoUrl(input)).toBeNull();
    },
  );
});

export interface ParsedRepoUrl {
  owner: string;
  name: string;
  /** Present when the URL points at a branch (`/tree/<branch>/...`). */
  branch?: string;
}

const SEGMENT = /^[A-Za-z0-9_.-]+$/;

/**
 * Parse the GitHub repository forms users paste (FE-UI-60):
 * `owner/repo`, `github.com/owner/repo`, `https://www.github.com/owner/repo/`,
 * `git@github.com:owner/repo.git` and `https://github.com/owner/repo/tree/dev/skills`.
 */
export function parseRepoUrl(input: string): ParsedRepoUrl | null {
  let cleaned = input.trim();
  if (!cleaned) return null;

  cleaned = cleaned
    .replace(/^git@github\.com:/i, "")
    .replace(/^(?:https?:\/\/)?(?:www\.)?github\.com\//i, "")
    .replace(/[?#].*$/, "")
    .replace(/\/+$/, "");

  const parts = cleaned.split("/").filter(Boolean);
  if (parts.length < 2) return null;

  const owner = parts[0];
  const name = parts[1].replace(/\.git$/i, "");
  if (!SEGMENT.test(owner) || !SEGMENT.test(name)) return null;

  if (parts.length === 2) return { owner, name };
  if (parts[2] === "tree" && parts[3]) {
    return { owner, name, branch: decodeURIComponent(parts[3]) };
  }
  return null;
}

import { axe } from "jest-axe";

/**
 * Runs axe's `button-name` rule against a rendered container and returns the
 * offending button HTML, so a failing assertion shows which button is unnamed.
 */
export async function findUnnamedButtons(
  container: Element,
): Promise<string[]> {
  const results = await axe(container, { runOnly: ["button-name"] });
  return results.violations.flatMap((violation) =>
    violation.nodes.map((node) => node.html),
  );
}

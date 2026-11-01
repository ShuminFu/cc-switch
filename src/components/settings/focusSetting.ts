/**
 * Find the control or section titled `title` inside `container`, scroll it
 * into view, focus it and flash a highlight (FE-UI-38). Looks for an element
 * whose aria-label is the title (ToggleRow switches) first, then a heading
 * with that text. Returns the focused element, or null when nothing matched.
 */
export function focusSettingByTitle(
  container: HTMLElement | null,
  title: string,
): HTMLElement | null {
  if (!container || !title) return null;
  const wanted = title.trim();
  const byLabel = Array.from(
    container.querySelectorAll<HTMLElement>("[aria-label]"),
  ).find((el) => el.getAttribute("aria-label")?.trim() === wanted);
  const heading = byLabel
    ? null
    : Array.from(
        container.querySelectorAll<HTMLElement>("h2, h3, h4, label, p"),
      ).find((el) => el.textContent?.trim() === wanted);
  const target = byLabel ?? heading;
  if (!target) return null;

  // Focus the control itself, or the accordion trigger / first control of
  // the section the heading belongs to.
  const focusable: HTMLElement =
    byLabel ??
    target.closest<HTMLElement>("button, [tabindex]") ??
    target.parentElement?.querySelector<HTMLElement>(
      "button, input, [role='switch'], [tabindex]",
    ) ??
    target;
  focusable.scrollIntoView?.({ block: "center", behavior: "smooth" });
  focusable.focus?.({ preventScroll: true });

  // A toggle row's <label>, or the accordion item / section of a heading.
  const highlight = byLabel
    ? (byLabel.closest<HTMLElement>("label") ?? byLabel)
    : (target.closest<HTMLElement>("[data-state], section") ?? target);
  highlight.classList.add("ring-2", "ring-primary", "ring-offset-2");
  setTimeout(
    () => highlight.classList.remove("ring-2", "ring-primary", "ring-offset-2"),
    1600,
  );
  return focusable;
}

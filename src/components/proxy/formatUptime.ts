/**
 * Localised uptime such as "1h 2m 3s" (en) or "1小时 2分钟 3秒" (zh), built
 * from Intl unit formatting instead of hard-coded English suffixes (FE-UI-23).
 */
export function formatUptime(totalSeconds: number, locale?: string): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;

  const unit = (value: number, name: "hour" | "minute" | "second") =>
    new Intl.NumberFormat(locale, {
      style: "unit",
      unit: name,
      unitDisplay: "narrow",
    }).format(value);

  if (hours > 0) {
    return [
      unit(hours, "hour"),
      unit(minutes, "minute"),
      unit(secs, "second"),
    ].join(" ");
  }
  if (minutes > 0) {
    return [unit(minutes, "minute"), unit(secs, "second")].join(" ");
  }
  return unit(secs, "second");
}

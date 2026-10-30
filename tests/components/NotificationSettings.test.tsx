import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { NotificationSettings } from "@/components/notifications/NotificationSettings";
import {
  DEFAULT_HEALTH_PROBE,
  DEFAULT_NOTIFICATION_TOGGLES,
  resolveHealthProbe,
  resolveNotificationToggles,
} from "@/components/notifications/notificationSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? key,
  }),
}));

const baseSettings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  language: "en",
} as unknown as SettingsFormState;

describe("notification settings defaults", () => {
  it("enables only failover and circuit notifications by default", () => {
    expect(resolveNotificationToggles(undefined)).toEqual({
      failover: true,
      circuitOpen: true,
      budgetExceeded: false,
      quotaExhausted: false,
      syncError: false,
    });
    expect(resolveNotificationToggles({ syncError: true })).toEqual({
      ...DEFAULT_NOTIFICATION_TOGGLES,
      syncError: true,
    });
  });

  it("keeps health probing opt-in", () => {
    expect(resolveHealthProbe(null)).toEqual(DEFAULT_HEALTH_PROBE);
    expect(DEFAULT_HEALTH_PROBE.enabled).toBe(false);
  });
});

describe("NotificationSettings", () => {
  it("renders defaults and saves the full toggle object", () => {
    const onChange = vi.fn();
    render(<NotificationSettings settings={baseSettings} onChange={onChange} />);

    const failover = screen.getByRole("switch", { name: "Failover switches" });
    const sync = screen.getByRole("switch", { name: "Sync errors" });
    expect(failover).toHaveAttribute("aria-checked", "true");
    expect(sync).toHaveAttribute("aria-checked", "false");

    fireEvent.click(sync);
    expect(onChange).toHaveBeenCalledWith({
      notifications: { ...DEFAULT_NOTIFICATION_TOGGLES, syncError: true },
    });
  });

  it("enables health probing with the default interval", () => {
    const onChange = vi.fn();
    render(<NotificationSettings settings={baseSettings} onChange={onChange} />);

    expect(screen.queryByText("Probe interval")).toBeNull();
    fireEvent.click(
      screen.getByRole("switch", { name: "Background health probing" }),
    );
    expect(onChange).toHaveBeenCalledWith({
      healthProbe: { enabled: true, intervalSecs: 120 },
    });
  });

  it("shows the interval picker when probing is enabled", () => {
    render(
      <NotificationSettings
        settings={
          {
            ...baseSettings,
            healthProbe: { enabled: true, intervalSecs: 300 },
          } as SettingsFormState
        }
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText("Probe interval")).toBeInTheDocument();
  });
});

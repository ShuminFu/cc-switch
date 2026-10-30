import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { BookOpen, CheckCircle2, Download, Plus, Sparkles } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { useProvidersQuery } from "@/lib/query";
import { settingsApi, type AppId } from "@/lib/api";
import { useProviderActions } from "@/hooks/useProviderActions";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { VisibleApps } from "@/types";
import { openFirstRunWizard, useFirstRun } from "./useFirstRun";
import {
  ONBOARDING_APPS,
  USER_MANUAL_URL,
  WIZARD_STEPS,
  detectedToolNames,
  importLiveConfig,
  initialSelection,
  selectedApps,
  type OnboardingApp,
  type WizardStep,
} from "./onboardingModel";

type ImportState = "idle" | "pending" | "imported" | "nothing" | "error";

function ProviderSetupRow({
  app,
  onAdd,
}: {
  app: OnboardingApp;
  onAdd: (appId: AppId) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data } = useProvidersQuery(app.id);
  const [importState, setImportState] = useState<ImportState>("idle");
  const [error, setError] = useState<string | null>(null);
  const count = Object.keys(data?.providers ?? {}).length;

  const handleImport = async () => {
    setImportState("pending");
    setError(null);
    try {
      const imported = await importLiveConfig(app.id);
      setImportState(imported ? "imported" : "nothing");
      await queryClient.invalidateQueries({ queryKey: ["providers", app.id] });
    } catch (err) {
      setImportState("error");
      setError(extractErrorMessage(err) || String(err));
    }
  };

  return (
    <div
      className="flex flex-wrap items-center gap-2 rounded-lg border border-border/60 px-3 py-2"
      data-testid={`onboarding-provider-${app.id}`}
    >
      <div className="min-w-[120px] flex-1">
        <div className="text-sm font-medium">
          {t(`apps.${app.id}`, { defaultValue: app.id })}
        </div>
        <div className="text-xs text-muted-foreground">
          {importState === "imported"
            ? t("onboarding.providers.imported", {
                defaultValue:
                  "Your existing configuration was saved as a provider named “default”.",
              })
            : importState === "nothing"
              ? t("onboarding.providers.nothingToImport", {
                  defaultValue: "No existing configuration was found.",
                })
              : importState === "error"
                ? error
                : count > 0
                  ? t("onboarding.providers.count", {
                      defaultValue: "{{count}} provider(s) configured",
                      count,
                    })
                  : t("onboarding.providers.none", {
                      defaultValue: "No providers yet",
                    })}
        </div>
      </div>
      {count > 0 && (
        <CheckCircle2 className="h-4 w-4 text-emerald-500" aria-hidden />
      )}
      {count === 0 && (
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-8 gap-1.5 text-xs"
          disabled={importState === "pending"}
          onClick={() => void handleImport()}
        >
          <Download className="h-3.5 w-3.5" />
          {t("onboarding.providers.import", {
            defaultValue: "Import current config",
          })}
        </Button>
      )}
      <Button
        type="button"
        size="sm"
        className="h-8 gap-1.5 text-xs"
        onClick={() => onAdd(app.id)}
      >
        <Plus className="h-3.5 w-3.5" />
        {t("onboarding.providers.add", { defaultValue: "Add provider" })}
      </Button>
    </div>
  );
}

function WizardAddProviderDialog({
  appId,
  onClose,
}: {
  appId: AppId;
  onClose: () => void;
}) {
  const { addProvider } = useProviderActions(appId);
  return (
    <AddProviderDialog
      open
      appId={appId}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      onSubmit={async (provider) => {
        await addProvider(provider);
        onClose();
      }}
    />
  );
}

/**
 * Guided first-run setup (DOCS-13): welcome → tools → providers → what's next.
 * Opens automatically on a fresh install (`firstRunNoticeConfirmed` unset) and
 * can be re-opened with `openFirstRunWizard()`. Every step can be skipped.
 */
export function FirstRunWizard() {
  const { t } = useTranslation();
  const { isOpen, settings, complete } = useFirstRun();
  const [step, setStep] = useState<WizardStep>("welcome");
  const [detected, setDetected] = useState<Set<string> | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [selection, setSelection] = useState<VisibleApps | null>(null);
  const [touched, setTouched] = useState(false);
  const [addingFor, setAddingFor] = useState<AppId | null>(null);

  // Reset to the first step every time the wizard opens.
  useEffect(() => {
    if (!isOpen) return;
    setStep("welcome");
    setTouched(false);
    setSelection(null);
    setDetected(null);
  }, [isOpen]);

  // Probe installed CLIs once per opening.
  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    setDetecting(true);
    settingsApi
      .getToolVersions(
        ONBOARDING_APPS.flatMap((app) => (app.tool ? [app.tool] : [])),
      )
      .then((probes) => {
        if (!cancelled) setDetected(detectedToolNames(probes ?? []));
      })
      .catch((error) => {
        console.warn("[FirstRunWizard] Tool detection failed", error);
        if (!cancelled) setDetected(new Set());
      })
      .finally(() => {
        if (!cancelled) setDetecting(false);
      });
    return () => {
      cancelled = true;
    };
  }, [isOpen]);

  const effectiveSelection = useMemo(
    () =>
      touched && selection
        ? selection
        : initialSelection(settings?.visibleApps, detected),
    [touched, selection, settings?.visibleApps, detected],
  );

  const chosenApps = selectedApps(effectiveSelection);
  const stepIndex = WIZARD_STEPS.indexOf(step);

  const toggleApp = (appId: keyof VisibleApps, checked: boolean) => {
    setTouched(true);
    setSelection({ ...effectiveSelection, [appId]: checked });
  };

  const skip = () => void complete();
  const finish = () => void complete({ visibleApps: effectiveSelection });
  const next = () => setStep(WIZARD_STEPS[Math.min(stepIndex + 1, 3)]);
  const back = () => setStep(WIZARD_STEPS[Math.max(stepIndex - 1, 0)]);

  const openManual = () => {
    void settingsApi.openExternal(USER_MANUAL_URL).catch((error) => {
      console.error("[FirstRunWizard] Failed to open manual", error);
    });
  };

  if (!isOpen) return null;

  return (
    <>
      <Dialog
        open={addingFor == null}
        onOpenChange={(open) => {
          if (!open) skip();
        }}
      >
        <DialogContent className="max-w-lg" zIndex="top">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Sparkles className="h-5 w-5 text-blue-500" />
              {t("firstRunNotice.title", {
                defaultValue: "Welcome to CC Switch",
              })}
            </DialogTitle>
            <p
              className="text-xs text-muted-foreground"
              data-testid="onboarding-step-indicator"
            >
              {t("onboarding.stepOf", {
                defaultValue: "Step {{current}} of {{total}}",
                current: stepIndex + 1,
                total: WIZARD_STEPS.length,
              })}
            </p>
          </DialogHeader>

          <div
            className="space-y-3 px-6 py-4"
            data-testid={`onboarding-${step}`}
          >
            {step === "welcome" && (
              <>
                <DialogDescription className="whitespace-pre-line leading-relaxed">
                  {t("onboarding.welcome.body", {
                    defaultValue:
                      "CC Switch manages providers, MCP servers, prompts and skills for Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw and Hermes. If you already had one of these tools configured, CC Switch saved that setup as a provider named “default”, so nothing is lost.",
                  })}
                </DialogDescription>
                <DialogDescription className="whitespace-pre-line leading-relaxed">
                  {t("firstRunNotice.bodyOfficial")}
                </DialogDescription>
                <DialogDescription>
                  {t("onboarding.welcome.wizardHint", {
                    defaultValue:
                      "This short setup picks the tools you use, imports your existing configs and helps you add a first provider. You can skip it and re-open it later from Settings → About.",
                  })}
                </DialogDescription>
              </>
            )}

            {step === "tools" && (
              <>
                <DialogDescription>
                  {t("onboarding.tools.description", {
                    defaultValue:
                      "Choose the tools you use. Only these appear on the home screen; you can change this later in Settings.",
                  })}
                </DialogDescription>
                <div className="grid gap-2 sm:grid-cols-2">
                  {ONBOARDING_APPS.map((app) => {
                    const isDetected =
                      app.tool != null && detected?.has(app.tool) === true;
                    const id = `onboarding-app-${app.id}`;
                    return (
                      <label
                        key={app.id}
                        htmlFor={id}
                        className="flex cursor-pointer items-center gap-2 rounded-lg border border-border/60 px-3 py-2 text-sm"
                      >
                        <Checkbox
                          id={id}
                          checked={effectiveSelection[app.id]}
                          onCheckedChange={(checked) =>
                            toggleApp(app.id, checked === true)
                          }
                        />
                        <span className="flex-1">
                          {t(`apps.${app.id}`, { defaultValue: app.id })}
                        </span>
                        {app.tool && (
                          <span className="text-[10px] text-muted-foreground">
                            {detecting && detected == null
                              ? t("onboarding.tools.detecting", {
                                  defaultValue: "Detecting…",
                                })
                              : isDetected
                                ? t("onboarding.tools.detected", {
                                    defaultValue: "Installed",
                                  })
                                : t("onboarding.tools.notDetected", {
                                    defaultValue: "Not found",
                                  })}
                          </span>
                        )}
                      </label>
                    );
                  })}
                </div>
                {chosenApps.length === 0 && (
                  <p className="text-xs text-red-500">
                    {t("onboarding.tools.pickOne", {
                      defaultValue: "Select at least one tool.",
                    })}
                  </p>
                )}
              </>
            )}

            {step === "providers" && (
              <>
                <DialogDescription>
                  {t("onboarding.providers.description", {
                    defaultValue:
                      "Import the configuration you already use, or add a provider from a preset. Each tool needs at least one provider to switch between.",
                  })}
                </DialogDescription>
                <div className="max-h-[50vh] space-y-2 overflow-y-auto">
                  {chosenApps.map((app) => (
                    <ProviderSetupRow
                      key={app.id}
                      app={app}
                      onAdd={setAddingFor}
                    />
                  ))}
                </div>
              </>
            )}

            {step === "done" && (
              <>
                <DialogDescription>
                  {t("onboarding.done.description", {
                    defaultValue:
                      "You're set. Restart any open terminal sessions so the tools pick up the new configuration.",
                  })}
                </DialogDescription>
                <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
                  <li>
                    {t("onboarding.done.proxy", {
                      defaultValue:
                        "Proxy & failover: route requests through the local proxy and fall back to the next provider automatically.",
                    })}
                  </li>
                  <li>
                    {t("onboarding.done.mcp", {
                      defaultValue:
                        "MCP, prompts and skills: manage them once and sync them to every tool.",
                    })}
                  </li>
                  <li>
                    {t("onboarding.done.usage", {
                      defaultValue:
                        "Usage: track cost per provider and set daily or monthly budget limits.",
                    })}
                  </li>
                </ul>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8 gap-1.5 text-xs"
                  onClick={openManual}
                >
                  <BookOpen className="h-3.5 w-3.5" />
                  {t("onboarding.done.manual", {
                    defaultValue: "Open the user manual",
                  })}
                </Button>
              </>
            )}
          </div>

          <DialogFooter className="gap-2 sm:justify-between">
            <Button type="button" variant="ghost" onClick={skip}>
              {t("onboarding.skip", { defaultValue: "Skip setup" })}
            </Button>
            <div className="flex gap-2">
              {stepIndex > 0 && (
                <Button type="button" variant="outline" onClick={back}>
                  {t("onboarding.back", { defaultValue: "Back" })}
                </Button>
              )}
              {step === "done" ? (
                <Button type="button" onClick={finish}>
                  {t("onboarding.finish", { defaultValue: "Finish" })}
                </Button>
              ) : (
                <Button
                  type="button"
                  onClick={next}
                  disabled={step === "tools" && chosenApps.length === 0}
                >
                  {step === "welcome"
                    ? t("onboarding.start", { defaultValue: "Start setup" })
                    : t("onboarding.next", { defaultValue: "Next" })}
                </Button>
              )}
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {addingFor != null && (
        <WizardAddProviderDialog
          appId={addingFor}
          onClose={() => setAddingFor(null)}
        />
      )}
    </>
  );
}

/** Small button that re-opens the setup wizard (used in Settings → About). */
export function ReopenFirstRunButton({ className }: { className?: string }) {
  const { t } = useTranslation();
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className={className ?? "h-8 gap-1.5 text-xs"}
      onClick={openFirstRunWizard}
    >
      <Sparkles className="h-3.5 w-3.5" />
      {t("onboarding.reopen", { defaultValue: "Setup guide" })}
    </Button>
  );
}

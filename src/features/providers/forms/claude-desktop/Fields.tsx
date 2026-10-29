import type { Dispatch, SetStateAction } from "react";
import { useFormContext } from "react-hook-form";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  FormControl,
  FormField,
  FormItem,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { CopilotAuthSection } from "@/components/providers/forms/CopilotAuthSection";
import { ApiKeySection } from "@/components/providers/forms/shared/ApiKeySection";
import { EndpointField } from "@/components/providers/forms/shared/EndpointField";
import { ModelDropdown } from "@/components/providers/forms/shared/ModelDropdown";
import type { FetchedModel } from "@/lib/api/model-fetch";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ClaudeApiFormat, ProviderCategory } from "@/types";
import {
  createRouteRow,
  routeRoleFromId,
  type RouteRow,
  type RouteRowValues,
} from "./routes";

export interface ClaudeDesktopFieldsProps {
  usesManagedOAuth: boolean;
  activeProviderType?: string;
  selectedGitHubAccountId: string | null;
  setSelectedGitHubAccountId: (accountId: string | null) => void;
  selectedCodexAccountId: string | null;
  setSelectedCodexAccountId: (accountId: string | null) => void;
  codexFastMode: boolean;
  setCodexFastMode: (enabled: boolean) => void;
  apiKey: string;
  setApiKey: (value: string) => void;
  apiKeyLinkCategory?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  apiKeyLinkWebsiteUrl: string;
  apiKeyLinkIsPartner?: boolean;
  apiKeyLinkPromotionKey?: string;
  baseUrl: string;
  setBaseUrl: (value: string) => void;
  needsModelMapping: boolean;
  handleModelMappingChange: (checked: boolean) => void;
  apiFormat: ClaudeApiFormat;
  setApiFormat: (format: ClaudeApiFormat) => void;
  routes: RouteRow[];
  setRoutes: Dispatch<SetStateAction<RouteRow[]>>;
  fetchedModels: FetchedModel[];
  isFetchingModels: boolean;
  handleFetchModels: () => void;
  directModelsExpanded: boolean;
  setDirectModelsExpanded: (expanded: boolean) => void;
}

/**
 * 非官方 Claude Desktop 供应商的字段：认证、端点、模型映射开关，
 * 以及 proxy 模式的四档路由表 / direct 模式的模型列表。
 */
export function ClaudeDesktopFields({
  usesManagedOAuth,
  activeProviderType,
  selectedGitHubAccountId,
  setSelectedGitHubAccountId,
  selectedCodexAccountId,
  setSelectedCodexAccountId,
  codexFastMode,
  setCodexFastMode,
  apiKey,
  setApiKey,
  apiKeyLinkCategory,
  shouldShowApiKeyLink,
  apiKeyLinkWebsiteUrl,
  apiKeyLinkIsPartner,
  apiKeyLinkPromotionKey,
  baseUrl,
  setBaseUrl,
  needsModelMapping,
  handleModelMappingChange,
  apiFormat,
  setApiFormat,
  routes,
  setRoutes,
  fetchedModels,
  isFetchingModels,
  handleFetchModels,
  directModelsExpanded,
  setDirectModelsExpanded,
}: ClaudeDesktopFieldsProps) {
  const { t } = useTranslation();
  const { control } = useFormContext<ProviderFormData>();

  const updateRoute = (index: number, patch: Partial<RouteRowValues>) => {
    setRoutes((current) =>
      current.map((row, i) => (i === index ? { ...row, ...patch } : row)),
    );
  };

  const renderActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex gap-1">
      {!usesManagedOAuth && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={handleFetchModels}
          disabled={isFetchingModels}
          className="h-7 gap-1"
        >
          {isFetchingModels ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Download className="h-3.5 w-3.5" />
          )}
          {t("providerForm.fetchModels")}
        </Button>
      )}
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onAdd}
        className="h-7 gap-1"
      >
        <Plus className="h-3.5 w-3.5" />
        {addLabel}
      </Button>
    </div>
  );

  return (
    <>
      {usesManagedOAuth ? (
        <div className="rounded-lg border border-border-default bg-muted/20 p-3">
          {activeProviderType === "github_copilot" ? (
            <CopilotAuthSection
              selectedAccountId={selectedGitHubAccountId}
              onAccountSelect={setSelectedGitHubAccountId}
            />
          ) : (
            <CodexOAuthSection
              selectedAccountId={selectedCodexAccountId}
              onAccountSelect={setSelectedCodexAccountId}
              fastModeEnabled={codexFastMode}
              onFastModeChange={setCodexFastMode}
            />
          )}
        </div>
      ) : (
        <ApiKeySection
          value={apiKey}
          onChange={setApiKey}
          category={apiKeyLinkCategory}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={apiKeyLinkWebsiteUrl}
          isPartner={apiKeyLinkIsPartner}
          partnerPromotionKey={apiKeyLinkPromotionKey}
        />
      )}

      <EndpointField
        id="baseUrl"
        label={t("providerForm.apiEndpoint")}
        value={baseUrl}
        onChange={(v) => setBaseUrl(v)}
        placeholder={t("providerForm.apiEndpointPlaceholder")}
        hint={
          needsModelMapping && apiFormat === "openai_responses"
            ? t("providerForm.apiHintResponses")
            : needsModelMapping && apiFormat === "openai_chat"
              ? t("providerForm.apiHintOAI")
              : needsModelMapping && apiFormat === "gemini_native"
                ? t("providerForm.apiHintGeminiNative")
                : t("providerForm.apiHint")
        }
        showManageButton={false}
      />

      <div className="space-y-3 rounded-lg border border-border-default bg-muted/20 p-4">
        <div className="flex items-center justify-between gap-4">
          <div className="space-y-1">
            <Label>{t("claudeDesktop.modelMappingToggle")}</Label>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {needsModelMapping
                ? t("claudeDesktop.modelMappingOnHint")
                : t("claudeDesktop.modelMappingOffHint")}
            </p>
          </div>
          <Switch
            checked={needsModelMapping}
            onCheckedChange={handleModelMappingChange}
            aria-label={t("claudeDesktop.modelMappingToggle")}
          />
        </div>
      </div>

      {needsModelMapping && (
        <div className="space-y-4 rounded-lg border border-border-default p-4">
          <div className="space-y-2">
            <Label>{t("providerForm.apiFormat")}</Label>
            <Select
              value={apiFormat}
              onValueChange={(value) => setApiFormat(value as ClaudeApiFormat)}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="anthropic">
                  {t("providerForm.apiFormatAnthropic")}
                </SelectItem>
                <SelectItem value="openai_chat">
                  {t("providerForm.apiFormatOpenAIChat")}
                </SelectItem>
                <SelectItem value="openai_responses">
                  {t("providerForm.apiFormatOpenAIResponses")}
                </SelectItem>
                <SelectItem value="gemini_native">
                  {t("providerForm.apiFormatGeminiNative")}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-3">
            <div className="space-y-1 border-t border-border-default pt-4">
              <div className="flex items-center justify-between">
                <Label>{t("claudeDesktop.routeMapTitle")}</Label>
                {!usesManagedOAuth && (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleFetchModels}
                    disabled={isFetchingModels}
                    className="h-7 gap-1"
                  >
                    {isFetchingModels ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Download className="h-3.5 w-3.5" />
                    )}
                    {t("providerForm.fetchModels")}
                  </Button>
                )}
              </div>
              <p className="text-xs leading-relaxed text-muted-foreground">
                {t("claudeDesktop.routeMapHint")}
              </p>
            </div>

            <div className="hidden grid-cols-[140px_1fr_1fr_116px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
              <span>{t("claudeDesktop.routeModelLabel")}</span>
              <span>{t("claudeDesktop.labelOverrideLabel")}</span>
              <span>{t("claudeDesktop.upstreamModelLabel")}</span>
              <span>{t("claudeDesktop.supports1mLabel")}</span>
            </div>
            {routes.map((route, index) => {
              const role = routeRoleFromId(route.route);
              const roleLabel =
                role === "opus"
                  ? t("claudeDesktop.routeRoleOpus")
                  : role === "haiku"
                    ? t("claudeDesktop.routeRoleHaiku")
                    : role === "fable"
                      ? t("claudeDesktop.routeRoleFable")
                      : t("claudeDesktop.routeRoleSonnet");
              // Haiku 档示范映射到轻量模型（flash），其余档映射到 pro；
              // 两列占位联动，保持每行「菜单显示名 ↔ 实际请求模型」品牌一致。
              const isHaikuRole = role === "haiku";
              const labelPlaceholder = isHaikuRole
                ? "DeepSeek V4 Flash"
                : "DeepSeek V4 Pro";
              const modelPlaceholder = isHaikuRole
                ? "deepseek-v4-flash"
                : "deepseek-v4-pro";
              return (
                <div
                  key={route.rowId}
                  className="grid grid-cols-1 gap-2 md:grid-cols-[140px_1fr_1fr_116px]"
                >
                  <div className="flex h-9 items-center rounded-md border border-input bg-muted px-3 text-sm font-medium text-muted-foreground">
                    {roleLabel}
                  </div>
                  <Input
                    value={route.labelOverride}
                    onChange={(event) =>
                      updateRoute(index, {
                        labelOverride: event.target.value,
                      })
                    }
                    placeholder={labelPlaceholder}
                  />
                  <div className="flex gap-1">
                    <Input
                      value={route.model}
                      onChange={(event) =>
                        updateRoute(index, { model: event.target.value })
                      }
                      placeholder={modelPlaceholder}
                      className="flex-1"
                    />
                    {fetchedModels.length > 0 && (
                      <ModelDropdown
                        models={fetchedModels}
                        onSelect={(id) =>
                          updateRoute(index, {
                            model: id,
                            labelOverride: route.labelOverride || id,
                          })
                        }
                      />
                    )}
                  </div>
                  <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                    <Checkbox
                      checked={route.supports1m}
                      onCheckedChange={(checked) =>
                        updateRoute(index, {
                          supports1m: checked === true,
                        })
                      }
                    />
                    {t("claudeDesktop.supports1mShort")}
                  </label>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {!needsModelMapping && (
        <Collapsible
          open={directModelsExpanded}
          onOpenChange={setDirectModelsExpanded}
        >
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
            >
              {directModelsExpanded ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              {t("claudeDesktop.directModelListTitle")}
            </Button>
          </CollapsibleTrigger>
          {!directModelsExpanded && (
            <p className="ml-1 mt-1 text-xs text-muted-foreground">
              {t("claudeDesktop.directModelListCollapsedHint")}
            </p>
          )}
          <CollapsibleContent className="space-y-4 pt-2">
            <div className="space-y-4 rounded-lg border border-border-default p-4">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <p className="flex-1 text-xs leading-relaxed text-muted-foreground">
                  {t("claudeDesktop.directModelListHint")}
                </p>
                {renderActionButtons(
                  () =>
                    setRoutes((current) => [
                      ...current,
                      createRouteRow({
                        route: "",
                        model: "",
                        labelOverride: "",
                        supports1m: false,
                      }),
                    ]),
                  t("claudeDesktop.addModel"),
                )}
              </div>

              {routes.length > 0 ? (
                <div className="space-y-2">
                  {routes.map((route, index) => (
                    <div
                      key={route.rowId}
                      className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_116px_36px]"
                    >
                      <div className="flex gap-1">
                        <Input
                          value={route.route}
                          onChange={(event) =>
                            updateRoute(index, {
                              route: event.target.value,
                            })
                          }
                          placeholder="claude-sonnet-4-6"
                          className="flex-1"
                        />
                        {fetchedModels.length > 0 && (
                          <ModelDropdown
                            models={fetchedModels}
                            onSelect={(id) => updateRoute(index, { route: id })}
                          />
                        )}
                      </div>
                      <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                        <Checkbox
                          checked={route.supports1m}
                          onCheckedChange={(checked) =>
                            updateRoute(index, {
                              supports1m: checked === true,
                            })
                          }
                        />
                        {t("claudeDesktop.supports1mShort")}
                      </label>
                      <Button
                        aria-label={t("common.delete")}
                        type="button"
                        variant="ghost"
                        size="icon"
                        onClick={() =>
                          setRoutes((current) =>
                            current.filter((_, i) => i !== index),
                          )
                        }
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                </div>
              ) : null}
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}

      <FormField
        control={control}
        name="settingsConfig"
        render={() => (
          <FormItem className="space-y-0">
            <FormControl>
              <input type="hidden" />
            </FormControl>
            <FormMessage />
          </FormItem>
        )}
      />
    </>
  );
}

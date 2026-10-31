import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import type { McpServerSpec } from "@/types";
import { maskEnvValue } from "@/components/env/maskEnvValue";
import { parseEnvText, parseHeadersText } from "./wizardParsers";

interface McpWizardModalProps {
  isOpen: boolean;
  onClose: () => void;
  onApply: (title: string, json: string) => void;
  initialTitle?: string;
  /** Lock the title (server ID), e.g. when editing an existing server. */
  titleReadOnly?: boolean;
  initialServer?: McpServerSpec;
}

/**
 * MCP 配置向导模态框
 * 帮助用户快速生成 MCP JSON 配置
 */
const McpWizardModal: React.FC<McpWizardModalProps> = ({
  isOpen,
  onClose,
  onApply,
  initialTitle,
  titleReadOnly = false,
  initialServer,
}) => {
  const { t } = useTranslation();
  const [wizardType, setWizardType] = useState<"stdio" | "http" | "sse">(
    "stdio",
  );
  const [wizardTitle, setWizardTitle] = useState("");
  // stdio 字段
  const [wizardCommand, setWizardCommand] = useState("");
  const [wizardArgs, setWizardArgs] = useState("");
  const [wizardEnv, setWizardEnv] = useState("");
  const [wizardCwd, setWizardCwd] = useState("");
  // http 和 sse 字段
  const [wizardUrl, setWizardUrl] = useState("");
  const [wizardHeaders, setWizardHeaders] = useState("");

  const envParse = parseEnvText(wizardEnv);
  const headersParse = parseHeadersText(wizardHeaders);
  const invalidEnvLines = wizardType === "stdio" ? envParse.invalidLines : [];
  const invalidHeaderLines =
    wizardType === "stdio" ? [] : headersParse.invalidLines;

  // 生成配置（预览时可遮蔽密钥）
  const buildConfig = (maskSecrets: boolean): McpServerSpec => {
    const config: McpServerSpec = {
      type: wizardType,
    };

    if (wizardType === "stdio") {
      // stdio 类型必需字段
      config.command = wizardCommand.trim();

      // 可选字段
      if (wizardArgs.trim()) {
        config.args = wizardArgs
          .split("\n")
          .map((s) => s.trim())
          .filter((s) => s.length > 0);
      }

      if (wizardCwd.trim()) {
        config.cwd = wizardCwd.trim();
      }

      const env = envParse.values;
      if (Object.keys(env).length > 0) {
        config.env = maskSecrets
          ? Object.fromEntries(
              Object.entries(env).map(([k, v]) => [k, maskEnvValue(k, v)]),
            )
          : env;
      }
    } else {
      // http 和 sse 类型必需字段
      config.url = wizardUrl.trim();

      // 可选字段
      const headers = headersParse.values;
      if (Object.keys(headers).length > 0) {
        config.headers = maskSecrets
          ? Object.fromEntries(
              Object.entries(headers).map(([k, v]) => [k, maskEnvValue(k, v)]),
            )
          : headers;
      }
    }

    return config;
  };

  const generatePreview = (maskSecrets = false): string =>
    JSON.stringify(buildConfig(maskSecrets), null, 2);

  const handleApply = () => {
    if (!wizardTitle.trim()) {
      toast.error(t("mcp.error.idRequired"), { duration: 3000 });
      return;
    }
    if (wizardType === "stdio" && !wizardCommand.trim()) {
      toast.error(t("mcp.error.commandRequired"), { duration: 3000 });
      return;
    }
    if ((wizardType === "http" || wizardType === "sse") && !wizardUrl.trim()) {
      toast.error(t("mcp.wizard.urlRequired"), { duration: 3000 });
      return;
    }
    if (invalidEnvLines.length > 0 || invalidHeaderLines.length > 0) {
      toast.error(
        t("mcp.wizard.invalidLines", {
          defaultValue: "Some lines could not be parsed; fix them first.",
        }),
        { duration: 3000 },
      );
      return;
    }

    const json = generatePreview();
    onApply(wizardTitle.trim(), json);
    handleClose();
  };

  const handleClose = () => {
    // 重置表单
    setWizardType("stdio");
    setWizardTitle("");
    setWizardCommand("");
    setWizardArgs("");
    setWizardEnv("");
    setWizardCwd("");
    setWizardUrl("");
    setWizardHeaders("");
    onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // Cmd+Enter on macOS, Ctrl+Enter elsewhere (FE-UI-32).
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleApply();
    }
  };

  useEffect(() => {
    if (!isOpen) return;

    const title = initialTitle ?? "";
    setWizardTitle(title);

    const resolvedType =
      initialServer?.type ?? (initialServer?.url ? "http" : "stdio");

    setWizardType(resolvedType);

    if (resolvedType === "http" || resolvedType === "sse") {
      setWizardUrl(initialServer?.url ?? "");
      const headersCandidate = initialServer?.headers;
      const headers =
        headersCandidate && typeof headersCandidate === "object"
          ? headersCandidate
          : undefined;
      setWizardHeaders(
        headers
          ? Object.entries(headers)
              .map(([k, v]) => `${k}: ${v ?? ""}`)
              .join("\n")
          : "",
      );
      setWizardCommand("");
      setWizardArgs("");
      setWizardEnv("");
      setWizardCwd("");
      return;
    }

    setWizardCommand(initialServer?.command ?? "");
    const argsValue = initialServer?.args;
    setWizardArgs(Array.isArray(argsValue) ? argsValue.join("\n") : "");
    const envCandidate = initialServer?.env;
    const env =
      envCandidate && typeof envCandidate === "object"
        ? envCandidate
        : undefined;
    setWizardEnv(
      env
        ? Object.entries(env)
            .map(([k, v]) => `${k}=${v ?? ""}`)
            .join("\n")
        : "",
    );
    setWizardCwd(
      typeof initialServer?.cwd === "string" ? initialServer.cwd : "",
    );
    setWizardUrl("");
    setWizardHeaders("");
  }, [isOpen]);

  const preview = generatePreview(true);

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && handleClose()}>
      <DialogContent
        className="max-w-2xl max-h-[90vh] flex flex-col"
        zIndex="alert"
      >
        <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
          <DialogTitle className="text-lg font-semibold">
            {t("mcp.wizard.title")}
          </DialogTitle>
        </DialogHeader>

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Hint */}
          <div className="rounded-lg border border-border-default bg-gray-100/50 dark:bg-gray-800/50 p-3">
            <p className="text-sm text-muted-foreground">
              {t("mcp.wizard.hint")}
            </p>
          </div>

          {/* Form Fields */}
          <div className="space-y-4 min-h-[400px]">
            {/* Type */}
            <div>
              <label className="mb-2 block text-sm font-medium text-foreground">
                {t("mcp.wizard.type")} <span className="text-red-500">*</span>
              </label>
              <div className="flex gap-4">
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="stdio"
                    checked={wizardType === "stdio"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http" | "sse")
                    }
                    className="w-4 h-4 accent-blue-500"
                  />
                  <span className="text-sm text-foreground">
                    {t("mcp.wizard.typeStdio")}
                  </span>
                </label>
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="http"
                    checked={wizardType === "http"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http" | "sse")
                    }
                    className="w-4 h-4 accent-blue-500"
                  />
                  <span className="text-sm text-foreground">
                    {t("mcp.wizard.typeHttp")}
                  </span>
                </label>
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="sse"
                    checked={wizardType === "sse"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http" | "sse")
                    }
                    className="w-4 h-4 accent-blue-500"
                  />
                  <span className="text-sm text-foreground">
                    {t("mcp.wizard.typeSse")}
                  </span>
                </label>
              </div>
            </div>

            {/* Title */}
            <div>
              <label className="mb-1 block text-sm font-medium text-foreground">
                {t("mcp.form.title")} <span className="text-red-500">*</span>
              </label>
              <Input
                type="text"
                value={wizardTitle}
                disabled={titleReadOnly}
                onChange={(e) => setWizardTitle(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t("mcp.form.titlePlaceholder")}
                className="font-mono"
              />
            </div>

            {/* Stdio 类型字段 */}
            {wizardType === "stdio" && (
              <>
                {/* Command */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.command")}{" "}
                    <span className="text-red-500">*</span>
                  </label>
                  <Input
                    type="text"
                    value={wizardCommand}
                    onChange={(e) => setWizardCommand(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={t("mcp.wizard.commandPlaceholder")}
                    className="font-mono"
                  />
                </div>

                {/* Args */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.args")}
                  </label>
                  <textarea
                    value={wizardArgs}
                    onChange={(e) => setWizardArgs(e.target.value)}
                    placeholder={t("mcp.wizard.argsPlaceholder")}
                    rows={3}
                    className="w-full rounded-md border border-border-default bg-background px-3 py-2 text-sm font-mono text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 resize-y"
                  />
                </div>

                {/* Env */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.env")}
                  </label>
                  <textarea
                    value={wizardEnv}
                    onChange={(e) => setWizardEnv(e.target.value)}
                    placeholder={t("mcp.wizard.envPlaceholder")}
                    rows={3}
                    aria-invalid={invalidEnvLines.length > 0}
                    className="w-full rounded-md border border-border-default bg-background px-3 py-2 text-sm font-mono text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 resize-y"
                  />
                  <InvalidLines lines={invalidEnvLines} />
                </div>

                {/* Working directory */}
                <div>
                  <label
                    htmlFor="mcp-wizard-cwd"
                    className="mb-1 block text-sm font-medium text-foreground"
                  >
                    {t("mcp.wizard.cwd", {
                      defaultValue: "Working directory",
                    })}
                  </label>
                  <Input
                    id="mcp-wizard-cwd"
                    type="text"
                    value={wizardCwd}
                    onChange={(e) => setWizardCwd(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder="/path/to/project"
                    className="font-mono"
                  />
                </div>
              </>
            )}

            {/* HTTP 和 SSE 类型字段 */}
            {(wizardType === "http" || wizardType === "sse") && (
              <>
                {/* URL */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.url")}{" "}
                    <span className="text-red-500">*</span>
                  </label>
                  <Input
                    type="text"
                    value={wizardUrl}
                    onChange={(e) => setWizardUrl(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={t("mcp.wizard.urlPlaceholder")}
                    className="font-mono"
                  />
                </div>

                {/* Headers */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.headers")}
                  </label>
                  <textarea
                    value={wizardHeaders}
                    onChange={(e) => setWizardHeaders(e.target.value)}
                    placeholder={t("mcp.wizard.headersPlaceholder")}
                    rows={3}
                    aria-invalid={invalidHeaderLines.length > 0}
                    className="w-full rounded-md border border-border-default bg-background px-3 py-2 text-sm font-mono text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 resize-y"
                  />
                  <InvalidLines lines={invalidHeaderLines} />
                </div>
              </>
            )}
          </div>

          {/* Preview */}
          {(wizardCommand ||
            wizardArgs ||
            wizardEnv ||
            wizardCwd ||
            wizardUrl ||
            wizardHeaders) && (
            <div className="space-y-2 border-t border-border-default pt-4">
              <h3 className="text-sm font-medium text-foreground">
                {t("mcp.wizard.preview")}
              </h3>
              <pre className="overflow-x-auto rounded-lg bg-gray-100 dark:bg-gray-800 p-3 text-xs font-mono text-gray-700 dark:text-gray-300">
                {preview}
              </pre>
            </div>
          )}
        </div>

        {/* Footer */}
        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={handleClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="mcp" onClick={handleApply}>
            <Save className="h-4 w-4" />
            {t("mcp.wizard.apply")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

const InvalidLines: React.FC<{ lines: string[] }> = ({ lines }) => {
  const { t } = useTranslation();
  if (lines.length === 0) return null;
  return (
    <p className="mt-1 text-xs text-destructive" role="alert">
      {t("mcp.wizard.unparsedLines", {
        defaultValue: "Cannot parse: {{lines}}",
        lines: lines.join(", "),
      })}
    </p>
  );
};

export default McpWizardModal;

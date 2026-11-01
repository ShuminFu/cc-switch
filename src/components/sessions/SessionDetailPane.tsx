import type { ComponentProps, RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { Virtualizer } from "@tanstack/react-virtual";
import {
  Clock,
  Copy,
  FileText,
  FolderOpen,
  MessageSquare,
  Play,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { SessionExportButton } from "@/components/session-export/SessionExportButton";
import { SessionCostBadge } from "@/components/workbench/SessionCostBadge";
import type { SessionMessage, SessionMeta } from "@/types";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardHeader, CardContent } from "@/components/ui/card";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { extractErrorMessage } from "@/utils/errorUtils";
import { ProviderIcon } from "@/components/ProviderIcon";
import { SessionMessageItem } from "./SessionMessageItem";
import { SessionTocDialog, SessionTocSidebar } from "./SessionToc";
import {
  formatSessionTitle,
  formatTimestamp,
  getBaseName,
  getProviderIconName,
  getProviderLabel,
} from "./utils";

type TocItems = ComponentProps<typeof SessionTocSidebar>["items"];

export interface SessionDetailPaneProps {
  detailRef: RefObject<HTMLDivElement>;
  scrollContainerRef: RefObject<HTMLDivElement>;
  selectedSession: SessionMeta | null;
  messages: SessionMessage[];
  isLoadingMessages: boolean;
  messagesError: unknown;
  refetchMessages: () => unknown;
  virtualizer: Virtualizer<HTMLDivElement, Element>;
  activeMessageIndex: number | null;
  detailSearchQuery: string;
  isDeleting: boolean;
  userMessagesToc: TocItems;
  tocDialogOpen: boolean;
  setTocDialogOpen: (open: boolean) => void;
  scrollToMessage: (index: number) => void;
  handleCopy: (text: string, successMessage: string) => Promise<void> | void;
  handleMessageCopy: (content: string) => Promise<void> | void;
  handleResume: () => Promise<void>;
  setDeleteTargets: (targets: SessionMeta[] | null) => void;
}

/**
 * Right-hand pane of the session manager: header with metadata and actions,
 * the virtualized transcript and its table of contents (SESS-25).
 */
export function SessionDetailPane({
  detailRef,
  scrollContainerRef,
  selectedSession,
  messages,
  isLoadingMessages,
  messagesError,
  refetchMessages,
  virtualizer,
  activeMessageIndex,
  detailSearchQuery,
  isDeleting,
  userMessagesToc,
  tocDialogOpen,
  setTocDialogOpen,
  scrollToMessage,
  handleCopy,
  handleMessageCopy,
  handleResume,
  setDeleteTargets,
}: SessionDetailPaneProps) {
  const { t } = useTranslation();
  return (
    <Card className="flex flex-col overflow-hidden min-h-0" ref={detailRef}>
      {!selectedSession ? (
        <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground p-8">
          <MessageSquare className="size-12 mb-3 opacity-30" />
          <p className="text-sm">{t("sessionManager.selectSession")}</p>
        </div>
      ) : (
        <>
          {/* 详情头部 */}
          <CardHeader className="py-3 px-4 border-b shrink-0">
            <div className="flex items-start justify-between gap-4">
              {/* 左侧：会话信息 */}
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 mb-1">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span className="shrink-0">
                        <ProviderIcon
                          icon={getProviderIconName(selectedSession.providerId)}
                          name={selectedSession.providerId}
                          size={20}
                        />
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>
                      {getProviderLabel(selectedSession.providerId, t)}
                    </TooltipContent>
                  </Tooltip>
                  <h2 className="text-base font-semibold truncate">
                    {formatSessionTitle(selectedSession)}
                  </h2>
                </div>

                {/* 元信息 */}
                <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                  <div className="flex items-center gap-1">
                    <Clock className="size-3" />
                    <span>
                      {formatTimestamp(
                        selectedSession.lastActiveAt ??
                          selectedSession.createdAt,
                      )}
                    </span>
                  </div>
                  {selectedSession.projectDir && (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          onClick={() =>
                            void handleCopy(
                              selectedSession.projectDir!,
                              t("sessionManager.projectDirCopied"),
                            )
                          }
                          className="flex items-center gap-1 hover:text-foreground transition-colors"
                        >
                          <FolderOpen className="size-3" />
                          <span className="truncate max-w-[200px]">
                            {getBaseName(selectedSession.projectDir)}
                          </span>
                        </button>
                      </TooltipTrigger>
                      <TooltipContent side="bottom" className="max-w-xs">
                        <p className="font-mono text-xs break-all">
                          {selectedSession.projectDir}
                        </p>
                        <p className="text-muted-foreground mt-1">
                          {t("sessionManager.clickToCopyPath")}
                        </p>
                      </TooltipContent>
                    </Tooltip>
                  )}
                  {selectedSession.sourcePath && (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          onClick={() =>
                            void handleCopy(
                              selectedSession.sourcePath!,
                              t("sessionManager.sourcePathCopied"),
                            )
                          }
                          className="flex items-center gap-1 hover:text-foreground transition-colors"
                        >
                          <FileText className="size-3 shrink-0" />
                          <span className="font-mono truncate max-w-[200px]">
                            {getBaseName(selectedSession.sourcePath)}
                          </span>
                        </button>
                      </TooltipTrigger>
                      <TooltipContent side="bottom" className="max-w-xs">
                        <p className="font-mono text-xs break-all">
                          {selectedSession.sourcePath}
                        </p>
                        <p className="text-muted-foreground mt-1">
                          {t("sessionManager.clickToCopyPath")}
                        </p>
                      </TooltipContent>
                    </Tooltip>
                  )}
                </div>
              </div>

              {/* 右侧：操作按钮组 */}
              <div className="flex items-center gap-2 shrink-0">
                <SessionCostBadge sessionId={selectedSession.sessionId} />
                <SessionExportButton
                  session={selectedSession}
                  messages={messages}
                  disabled={isLoadingMessages}
                />
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      size="sm"
                      className="gap-1.5"
                      onClick={() => void handleResume()}
                      disabled={!selectedSession.resumeCommand}
                    >
                      <Play className="size-3.5" />
                      <span className="hidden sm:inline">
                        {t("sessionManager.resume", {
                          defaultValue: "恢复会话",
                        })}
                      </span>
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    {selectedSession.resumeCommand
                      ? t("sessionManager.resumeTooltip", {
                          defaultValue: "在终端中恢复此会话",
                        })
                      : t("sessionManager.noResumeCommand", {
                          defaultValue: "此会话无法恢复",
                        })}
                  </TooltipContent>
                </Tooltip>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      size="sm"
                      variant="destructive"
                      className="gap-1.5"
                      onClick={() => setDeleteTargets([selectedSession])}
                      disabled={!selectedSession.sourcePath || isDeleting}
                    >
                      <Trash2 className="size-3.5" />
                      <span className="hidden sm:inline">
                        {isDeleting
                          ? t("sessionManager.deleting", {
                              defaultValue: "删除中...",
                            })
                          : t("sessionManager.delete", {
                              defaultValue: "删除会话",
                            })}
                      </span>
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    {t("sessionManager.deleteTooltip", {
                      defaultValue: "永久删除此本地会话记录",
                    })}
                  </TooltipContent>
                </Tooltip>
              </div>
            </div>

            {/* 恢复命令预览 */}
            {selectedSession.resumeCommand && (
              <div className="mt-3 flex items-center gap-2">
                <div className="flex-1 rounded-md bg-muted/60 px-3 py-1.5 font-mono text-xs text-muted-foreground truncate">
                  {selectedSession.resumeCommand}
                </div>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-7 shrink-0"
                      onClick={() =>
                        void handleCopy(
                          selectedSession.resumeCommand!,
                          t("sessionManager.resumeCommandCopied"),
                        )
                      }
                    >
                      <Copy className="size-3.5" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    {t("sessionManager.copyCommand", {
                      defaultValue: "复制命令",
                    })}
                  </TooltipContent>
                </Tooltip>
              </div>
            )}
          </CardHeader>

          {/* 消息列表区域 */}
          <CardContent className="flex-1 min-h-0 p-0">
            <div className="flex h-full min-w-0">
              {/* 消息列表 */}
              <div className="flex-1 min-w-0 flex flex-col">
                <div className="px-4 pt-4 pb-2 min-w-0">
                  <div className="flex items-center gap-2">
                    <MessageSquare className="size-4 text-muted-foreground" />
                    <span className="text-sm font-medium">
                      {t("sessionManager.conversationHistory", {
                        defaultValue: "对话记录",
                      })}
                    </span>
                    <Badge variant="secondary" className="text-xs">
                      {messages.length}
                    </Badge>
                  </div>
                </div>
                <div
                  ref={scrollContainerRef}
                  className="flex-1 overflow-y-auto px-4 pb-4 min-w-0"
                >
                  {isLoadingMessages ? (
                    <div className="flex items-center justify-center py-12">
                      <RefreshCw className="size-5 animate-spin text-muted-foreground" />
                    </div>
                  ) : messagesError ? (
                    <div
                      role="alert"
                      className="flex flex-col items-center justify-center gap-2 py-12 text-center"
                    >
                      <p className="text-sm text-destructive">
                        {t("sessionManager.messagesLoadFailed", {
                          defaultValue: "Failed to load this session",
                        })}
                      </p>
                      <p className="max-w-md break-all text-xs text-muted-foreground">
                        {extractErrorMessage(messagesError)}
                      </p>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => void refetchMessages()}
                      >
                        {t("common.retry", { defaultValue: "Retry" })}
                      </Button>
                    </div>
                  ) : messages.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-12 text-center">
                      <MessageSquare className="size-8 text-muted-foreground/50 mb-2" />
                      <p className="text-sm text-muted-foreground">
                        {t("sessionManager.emptySession")}
                      </p>
                    </div>
                  ) : (
                    <div
                      style={{
                        height: virtualizer.getTotalSize(),
                        position: "relative",
                      }}
                    >
                      {virtualizer.getVirtualItems().map((virtualRow) => (
                        <div
                          key={virtualRow.key}
                          data-index={virtualRow.index}
                          ref={virtualizer.measureElement}
                          style={{
                            position: "absolute",
                            top: 0,
                            left: 0,
                            width: "100%",
                            transform: `translateY(${virtualRow.start}px)`,
                          }}
                        >
                          <SessionMessageItem
                            message={messages[virtualRow.index]}
                            isActive={activeMessageIndex === virtualRow.index}
                            searchQuery={detailSearchQuery}
                            onCopy={handleMessageCopy}
                          />
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>

              {/* 右侧目录 - 类似少数派 (大屏幕) */}
              <SessionTocSidebar
                items={userMessagesToc}
                onItemClick={scrollToMessage}
              />
            </div>

            {/* 浮动目录按钮 (小屏幕) */}
            <SessionTocDialog
              items={userMessagesToc}
              onItemClick={scrollToMessage}
              open={tocDialogOpen}
              onOpenChange={setTocDialogOpen}
            />
          </CardContent>
        </>
      )}
    </Card>
  );
}

import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Server } from "lucide-react";
import {
  useAllMcpServers,
  useToggleMcpApp,
  useDeleteMcpServer,
  useImportMcpFromApps,
} from "@/hooks/useMcp";
import type { McpServer } from "@/types";
import type { AppId } from "@/lib/api/types";
import McpFormModal from "./McpFormModal";
import { Edit3, Trash2, ExternalLink } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { mcpPresets } from "@/config/mcpPresets";
import { toast } from "sonner";
import { MCP_APP_IDS } from "@/config/appConfig";
import { ManagedItemsPanel } from "@/components/common/ManagedItemsPanel";
import { useConfirm } from "@/components/common/useConfirm";
import { McpSyncSkipBadges } from "./McpSyncSkipBadges";
import { useMcpSyncSkips } from "@/lib/query/adapters";

interface UnifiedMcpPanelProps {
  onOpenChange: (open: boolean) => void;
}

export interface UnifiedMcpPanelHandle {
  openAdd: () => void;
  openImport: () => void;
}

const UnifiedMcpPanel = React.forwardRef<
  UnifiedMcpPanelHandle,
  UnifiedMcpPanelProps
>(({ onOpenChange: _onOpenChange }, ref) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const confirm = useConfirm();

  const { data: serversMap, isLoading } = useAllMcpServers();
  const toggleAppMutation = useToggleMcpApp();
  const deleteServerMutation = useDeleteMcpServer();
  const importMutation = useImportMcpFromApps();
  const { data: syncSkips } = useMcpSyncSkips();

  const serverEntries = useMemo((): Array<[string, McpServer]> => {
    if (!serversMap) return [];
    return Object.entries(serversMap);
  }, [serversMap]);

  const handleToggleApp = async (
    serverId: string,
    app: AppId,
    enabled: boolean,
  ) => {
    try {
      await toggleAppMutation.mutateAsync({ serverId, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleEdit = (id: string) => {
    setEditingId(id);
    setIsFormOpen(true);
  };

  const handleAdd = () => {
    setEditingId(null);
    setIsFormOpen(true);
  };

  const handleImport = async () => {
    try {
      const count = await importMutation.mutateAsync();
      if (count === 0) {
        toast.success(t("mcp.unifiedPanel.noImportFound"), {
          closeButton: true,
        });
      } else {
        toast.success(t("mcp.unifiedPanel.importSuccess", { count }), {
          closeButton: true,
        });
      }
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  React.useImperativeHandle(ref, () => ({
    openAdd: handleAdd,
    openImport: handleImport,
  }));

  const handleDelete = (id: string) => {
    confirm.confirm({
      title: t("mcp.unifiedPanel.deleteServer"),
      message: t("mcp.unifiedPanel.deleteConfirm", { id }),
      onConfirm: async () => {
        try {
          await deleteServerMutation.mutateAsync(id);
          toast.success(t("common.success"), { closeButton: true });
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
          throw error;
        }
      },
    });
  };

  const handleCloseForm = () => {
    setIsFormOpen(false);
    setEditingId(null);
  };

  return (
    <ManagedItemsPanel
      items={serverEntries}
      isLoading={isLoading}
      getId={getEntryId}
      totalLabel={t("mcp.serverCount", { count: serverEntries.length })}
      appIds={MCP_APP_IDS}
      banner={<McpSyncSkipBadges skips={syncSkips} />}
      getApps={getEntryApps}
      onToggleApp={([id], app, enabled) => handleToggleApp(id, app, enabled)}
      renderPrimary={([id, server]) => (
        <McpServerPrimary id={id} server={server} />
      )}
      getActions={([id]) => [
        {
          key: "edit",
          label: t("common.edit"),
          icon: <Edit3 size={14} />,
          onClick: () => handleEdit(id),
        },
        {
          key: "delete",
          label: t("common.delete"),
          icon: <Trash2 size={14} />,
          tone: "danger",
          onClick: () => handleDelete(id),
        },
      ]}
      loadingLabel={t("mcp.loading")}
      empty={{
        icon: <Server size={24} className="text-muted-foreground" />,
        title: t("mcp.unifiedPanel.noServers"),
        description: t("mcp.emptyDescription"),
      }}
      confirm={confirm}
    >
      {isFormOpen && (
        <McpFormModal
          editingId={editingId || undefined}
          initialData={
            editingId && serversMap ? serversMap[editingId] : undefined
          }
          existingIds={serversMap ? Object.keys(serversMap) : []}
          defaultFormat="json"
          onSave={async () => {
            setIsFormOpen(false);
            setEditingId(null);
          }}
          onClose={handleCloseForm}
        />
      )}
    </ManagedItemsPanel>
  );
});

UnifiedMcpPanel.displayName = "UnifiedMcpPanel";

type McpEntry = [string, McpServer];
const getEntryId = ([id]: McpEntry) => id;
const getEntryApps = ([, server]: McpEntry) => server.apps;

const McpServerPrimary: React.FC<{ id: string; server: McpServer }> = ({
  id,
  server,
}) => {
  const { t } = useTranslation();
  const name = server.name || id;
  const description = server.description || "";

  const meta = mcpPresets.find((p) => p.id === id);
  const docsUrl = server.docs || meta?.docs;
  const homepageUrl = server.homepage || meta?.homepage;
  const tags = server.tags || meta?.tags;

  const openDocs = async () => {
    const url = docsUrl || homepageUrl;
    if (!url) return;
    try {
      await settingsApi.openExternal(url);
    } catch {
      // ignore
    }
  };

  return (
    <>
      <div className="flex items-center gap-1.5">
        <span className="font-medium text-sm text-foreground truncate">
          {name}
        </span>
        {docsUrl && (
          <button
            type="button"
            onClick={openDocs}
            className="text-muted-foreground/60 hover:text-foreground flex-shrink-0"
            title={t("mcp.presets.docs")}
            aria-label={t("mcp.presets.docs")}
          >
            <ExternalLink size={12} />
          </button>
        )}
      </div>
      {description && (
        <p
          className="text-xs text-muted-foreground truncate"
          title={description}
        >
          {description}
        </p>
      )}
      {!description && tags && tags.length > 0 && (
        <p className="text-xs text-muted-foreground/60 truncate">
          {tags.join(", ")}
        </p>
      )}
    </>
  );
};

export default UnifiedMcpPanel;

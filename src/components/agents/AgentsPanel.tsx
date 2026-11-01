import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, Plus, AlertTriangle } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  claudeAgentsApi,
  type ClaudeAgentInfo,
  type NewClaudeAgent,
} from "@/lib/api/claudeAgents";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

/** Same rule as the backend: lowercase letters, digits and hyphens. */
const AGENT_NAME_PATTERN = /^[a-z0-9][a-z0-9-]{0,63}$/;

const EMPTY_AGENT: NewClaudeAgent = {
  name: "",
  description: "",
  model: "",
  tools: "",
  prompt: "",
};

/**
 * Claude Code subagents (PRODUCT-42, first increment): lists the user-level
 * agents in ~/.claude/agents and creates new ones with valid frontmatter.
 * Existing files are shown read-only; editing stays in your editor for now.
 */
export function AgentsPanel(_props: AgentsPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState<NewClaudeAgent>(EMPTY_AGENT);
  const [expanded, setExpanded] = useState<string | null>(null);

  const { data: agents = [], isLoading } = useQuery({
    queryKey: queryKeys.claudeAgents.all,
    queryFn: claudeAgentsApi.list,
  });

  const create = useMutation({
    mutationFn: claudeAgentsApi.create,
    onSuccess: async (agent) => {
      toast.success(
        t("agents.created", {
          defaultValue: "Agent {{name}} created",
          name: agent.name,
        }),
        { description: agent.path },
      );
      setCreating(false);
      setDraft(EMPTY_AGENT);
      await queryClient.invalidateQueries({
        queryKey: queryKeys.claudeAgents.all,
      });
    },
    onError: (error) =>
      toast.error(extractErrorMessage(error) || String(error)),
  });

  const nameValid = AGENT_NAME_PATTERN.test(draft.name);
  const canSave = nameValid && draft.description.trim().length > 0;
  const set =
    (field: keyof NewClaudeAgent) =>
    (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
      setDraft((prev) => ({ ...prev, [field]: e.target.value }));

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 gap-4">
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-muted-foreground">
          {t("agents.description", {
            defaultValue:
              "Claude Code subagents from ~/.claude/agents. Claude Code picks them up on its next start.",
          })}
        </p>
        <Button size="sm" className="gap-1.5" onClick={() => setCreating(true)}>
          <Plus className="h-4 w-4" />
          {t("agents.new", { defaultValue: "New agent" })}
        </Button>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto space-y-2 pb-4">
        {!isLoading && agents.length === 0 ? (
          <div className="glass-card rounded-xl p-8 flex flex-col items-center justify-center text-center space-y-3">
            <Bot className="w-10 h-10 text-muted-foreground" />
            <p className="text-muted-foreground">
              {t("agents.empty", {
                defaultValue: "No subagents yet. Create one to get started.",
              })}
            </p>
          </div>
        ) : (
          agents.map((agent: ClaudeAgentInfo) => (
            <div
              key={agent.fileName}
              className="rounded-lg border border-border bg-card/50 p-3"
            >
              <button
                type="button"
                className="flex w-full items-start justify-between gap-3 text-left"
                onClick={() =>
                  setExpanded((cur) =>
                    cur === agent.fileName ? null : agent.fileName,
                  )
                }
                aria-expanded={expanded === agent.fileName}
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{agent.name}</span>
                    {agent.model && (
                      <Badge variant="secondary">{agent.model}</Badge>
                    )}
                    {agent.invalid && (
                      <Badge
                        variant="outline"
                        className="gap-1 border-amber-500/50 text-amber-600 dark:text-amber-400"
                        title={t("agents.invalidHint", {
                          defaultValue:
                            "The frontmatter needs a name and a description, or Claude Code ignores this file.",
                        })}
                      >
                        <AlertTriangle className="h-3 w-3" />
                        {t("agents.invalid", {
                          defaultValue: "Invalid frontmatter",
                        })}
                      </Badge>
                    )}
                  </div>
                  {agent.description && (
                    <p className="text-sm text-muted-foreground line-clamp-2">
                      {agent.description}
                    </p>
                  )}
                </div>
                {agent.tools && (
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {agent.tools}
                  </span>
                )}
              </button>
              {expanded === agent.fileName && (
                <div className="mt-3 space-y-2">
                  <p
                    className="text-xs text-muted-foreground break-all"
                    title={agent.path}
                  >
                    {agent.path}
                  </p>
                  <pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded bg-muted p-2 text-xs">
                    {agent.prompt}
                  </pre>
                </div>
              )}
            </div>
          ))
        )}
      </div>

      <Dialog
        open={creating}
        onOpenChange={(open) => {
          if (!create.isPending) setCreating(open);
        }}
      >
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>
              {t("agents.new", { defaultValue: "New agent" })}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3 px-6">
            <div className="space-y-1">
              <Label htmlFor="agent-name">
                {t("agents.field.name", { defaultValue: "Name" })}
              </Label>
              <Input
                id="agent-name"
                value={draft.name}
                onChange={set("name")}
                placeholder="code-reviewer"
                aria-invalid={draft.name.length > 0 && !nameValid}
              />
              {draft.name.length > 0 && !nameValid && (
                <p className="text-xs text-destructive">
                  {t("agents.field.nameInvalid", {
                    defaultValue: "Use lowercase letters, digits and hyphens.",
                  })}
                </p>
              )}
            </div>
            <div className="space-y-1">
              <Label htmlFor="agent-description">
                {t("agents.field.description", {
                  defaultValue: "When to use this agent",
                })}
              </Label>
              <Input
                id="agent-description"
                value={draft.description}
                onChange={set("description")}
              />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1">
                <Label htmlFor="agent-model">
                  {t("agents.field.model", { defaultValue: "Model" })}
                </Label>
                <Input
                  id="agent-model"
                  value={draft.model}
                  onChange={set("model")}
                  placeholder="sonnet / opus / haiku / inherit"
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="agent-tools">
                  {t("agents.field.tools", { defaultValue: "Tools" })}
                </Label>
                <Input
                  id="agent-tools"
                  value={draft.tools}
                  onChange={set("tools")}
                  placeholder="Read, Grep, Bash"
                />
              </div>
            </div>
            <div className="space-y-1">
              <Label htmlFor="agent-prompt">
                {t("agents.field.prompt", { defaultValue: "System prompt" })}
              </Label>
              <Textarea
                id="agent-prompt"
                rows={8}
                value={draft.prompt}
                onChange={set("prompt")}
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setCreating(false)}
              disabled={create.isPending}
            >
              {t("common.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button
              onClick={() => create.mutate(draft)}
              disabled={!canSave || create.isPending}
            >
              {t("agents.create", { defaultValue: "Create agent" })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

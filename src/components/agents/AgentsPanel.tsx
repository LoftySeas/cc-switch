import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Archive, Bot, Pencil, Plus, RefreshCw, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { AgentFormDialog, type AgentFormResult } from "./AgentFormDialog";
import { ListItemRow } from "@/components/common/ListItemRow";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { Agent, AgentLifecycle, CreateAgentInput } from "@/lib/api/agents";
import {
  agentKeys,
  isAgentRevisionConflict,
  useAgentsQuery,
  useCreateAgentMutation,
  useRetireAgentMutation,
  useUpdateAgentMutation,
} from "@/lib/query/agents";
import { extractErrorMessage } from "@/utils/errorUtils";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

type LifecycleFilter = "all" | AgentLifecycle;

const lifecycleClasses: Record<AgentLifecycle, string> = {
  draft:
    "border-slate-500/25 bg-slate-500/10 text-slate-700 dark:text-slate-300",
  active:
    "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  suspended:
    "border-amber-500/25 bg-amber-500/10 text-amber-700 dark:text-amber-300",
  retired: "border-border-default bg-muted text-muted-foreground",
};

export function AgentsPanel(_props: AgentsPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const agentsQuery = useAgentsQuery();
  const createMutation = useCreateAgentMutation();
  const updateMutation = useUpdateAgentMutation();
  const retireMutation = useRetireAgentMutation();

  const [searchQuery, setSearchQuery] = useState("");
  const [lifecycleFilter, setLifecycleFilter] =
    useState<LifecycleFilter>("all");
  const [formAgentId, setFormAgentId] = useState<string | "new" | null>(null);
  const [retireAgentId, setRetireAgentId] = useState<string | null>(null);

  const agents = agentsQuery.data ?? [];
  const formAgent =
    formAgentId && formAgentId !== "new"
      ? agents.find((agent) => agent.id === formAgentId)
      : undefined;
  const retireAgent = retireAgentId
    ? agents.find((agent) => agent.id === retireAgentId)
    : undefined;

  const filteredAgents = useMemo(() => {
    const query = searchQuery.trim().toLocaleLowerCase();
    return agents.filter((agent) => {
      const matchesLifecycle =
        lifecycleFilter === "all" || agent.lifecycleState === lifecycleFilter;
      const matchesQuery =
        !query ||
        [agent.name, agent.description, agent.owner, agent.id].some((value) =>
          value.toLocaleLowerCase().includes(query),
        );
      return matchesLifecycle && matchesQuery;
    });
  }, [agents, lifecycleFilter, searchQuery]);

  const showError = (error: unknown) => {
    toast.error(t("agents.operationFailed"), {
      description: extractErrorMessage(error) || t("common.unknown"),
      closeButton: true,
    });
  };

  const handleFormSubmit = async (
    input: CreateAgentInput,
    expectedRevision?: number,
  ): Promise<AgentFormResult> => {
    try {
      if (formAgentId === "new") {
        await createMutation.mutateAsync(input);
        toast.success(t("agents.createSuccess"), { closeButton: true });
      } else if (formAgent && expectedRevision !== undefined) {
        await updateMutation.mutateAsync({
          id: formAgent.id,
          input: { ...input, expectedRevision },
        });
        toast.success(t("agents.updateSuccess"), { closeButton: true });
      } else {
        return "error";
      }
      setFormAgentId(null);
      return "success";
    } catch (error) {
      if (isAgentRevisionConflict(error)) {
        await queryClient.invalidateQueries({ queryKey: agentKeys.all });
        toast.warning(t("agents.conflictTitle"), { closeButton: true });
        return "conflict";
      }
      showError(error);
      return "error";
    }
  };

  const reloadLatest = async (id: string): Promise<Agent | undefined> => {
    const result = await agentsQuery.refetch();
    return result.data?.find((agent) => agent.id === id);
  };

  const handleRetire = async () => {
    if (!retireAgent || retireAgent.lifecycleState === "retired") return;
    try {
      await retireMutation.mutateAsync({
        id: retireAgent.id,
        revision: retireAgent.revision,
      });
      setRetireAgentId(null);
      toast.success(t("agents.retireSuccess"), { closeButton: true });
    } catch (error) {
      if (isAgentRevisionConflict(error)) {
        await queryClient.invalidateQueries({ queryKey: agentKeys.all });
        toast.warning(t("agents.retireConflict"), { closeButton: true });
        return;
      }
      showError(error);
    }
  };

  const mutationPending =
    createMutation.isPending ||
    updateMutation.isPending ||
    retireMutation.isPending;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-6">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <p className="text-sm text-muted-foreground">
            {t("agents.registryDescription")}
          </p>
          <p className="mt-1 text-xs font-medium text-muted-foreground">
            {t("agents.count", { count: agents.length })}
          </p>
        </div>
        <Button
          onClick={() => setFormAgentId("new")}
          disabled={mutationPending}
        >
          <Plus className="mr-2 h-4 w-4" />
          {t("agents.create")}
        </Button>
      </div>

      <div className="mb-4 flex flex-col gap-2 sm:flex-row">
        <ManagementListSearch
          value={searchQuery}
          onValueChange={setSearchQuery}
          placeholder={t("agents.searchPlaceholder")}
          ariaLabel={t("agents.searchAriaLabel")}
          clearLabel={t("common.clear")}
          className="mb-0 flex-1"
        />
        <Select
          value={lifecycleFilter}
          onValueChange={(value) =>
            setLifecycleFilter(value as LifecycleFilter)
          }
        >
          <SelectTrigger
            className="w-full sm:w-44"
            aria-label={t("agents.filterLabel")}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("agents.allStates")}</SelectItem>
            {(["draft", "active", "suspended", "retired"] as const).map(
              (state) => (
                <SelectItem key={state} value={state}>
                  {t(`agents.lifecycle.${state}`)}
                </SelectItem>
              ),
            )}
          </SelectContent>
        </Select>
      </div>

      <ScrollArea type="auto" className="-mr-3 min-h-0 flex-1">
        <div className="pr-3 pb-24">
          {agentsQuery.isLoading ? (
            <div className="py-12 text-center text-sm text-muted-foreground">
              {t("agents.loading")}
            </div>
          ) : agentsQuery.isError ? (
            <div className="flex flex-col items-center py-12 text-center">
              <RefreshCw className="mb-3 h-8 w-8 text-muted-foreground/60" />
              <p className="text-sm text-muted-foreground">
                {t("agents.loadFailed")}
              </p>
              <Button
                variant="outline"
                size="sm"
                className="mt-4"
                onClick={() => agentsQuery.refetch()}
              >
                {t("agents.retry")}
              </Button>
            </div>
          ) : agents.length === 0 ? (
            <div className="flex flex-col items-center py-12 text-center">
              <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-muted">
                <Bot className="h-6 w-6 text-muted-foreground" />
              </div>
              <h3 className="text-base font-medium">
                {t("agents.emptyTitle")}
              </h3>
              <p className="mt-1 max-w-sm text-sm text-muted-foreground">
                {t("agents.emptyDescription")}
              </p>
            </div>
          ) : filteredAgents.length === 0 ? (
            <div className="flex flex-col items-center py-12 text-center text-muted-foreground">
              <Search className="mb-3 h-8 w-8 opacity-50" />
              <p className="text-sm">{t("agents.noResults")}</p>
            </div>
          ) : (
            <div className="overflow-hidden rounded-xl border border-border-default">
              {filteredAgents.map((agent, index) => {
                const retired = agent.lifecycleState === "retired";
                return (
                  <ListItemRow
                    key={agent.id}
                    isLast={index === filteredAgents.length - 1}
                  >
                    <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted">
                      <Bot className="h-4 w-4 text-muted-foreground" />
                    </div>
                    <div className="min-w-0 flex-1 py-0.5">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="truncate text-sm font-medium">
                          {agent.name}
                        </span>
                        <Badge
                          variant="outline"
                          className={`px-2 py-0 text-[11px] font-medium ${lifecycleClasses[agent.lifecycleState]}`}
                        >
                          {t(`agents.lifecycle.${agent.lifecycleState}`)}
                        </Badge>
                      </div>
                      {agent.description ? (
                        <p className="mt-0.5 truncate text-xs text-muted-foreground">
                          {agent.description}
                        </p>
                      ) : null}
                      <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-[11px] text-muted-foreground/80">
                        <span>
                          {t("agents.ownerValue", { owner: agent.owner })}
                        </span>
                        <span className="font-mono">{agent.id}</span>
                        <span>
                          {t("agents.revision", { revision: agent.revision })}
                        </span>
                      </div>
                    </div>
                    {retired ? (
                      <span className="hidden text-xs text-muted-foreground sm:inline">
                        {t("agents.retiredReadOnly")}
                      </span>
                    ) : (
                      <div className="flex shrink-0 items-center gap-1 sm:opacity-0 sm:transition-opacity sm:group-hover:opacity-100 sm:focus-within:opacity-100">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          title={t("agents.edit")}
                          aria-label={t("agents.editAgent", {
                            name: agent.name,
                          })}
                          disabled={mutationPending}
                          onClick={() => setFormAgentId(agent.id)}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-destructive hover:text-destructive"
                          title={t("agents.retire")}
                          aria-label={t("agents.retireAgent", {
                            name: agent.name,
                          })}
                          disabled={mutationPending}
                          onClick={() => setRetireAgentId(agent.id)}
                        >
                          <Archive className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    )}
                  </ListItemRow>
                );
              })}
            </div>
          )}
        </div>
      </ScrollArea>

      {formAgentId && (formAgentId === "new" || formAgent) ? (
        <AgentFormDialog
          key={formAgentId}
          agent={formAgent}
          pending={createMutation.isPending || updateMutation.isPending}
          onClose={() => setFormAgentId(null)}
          onSubmit={handleFormSubmit}
          onReloadLatest={reloadLatest}
        />
      ) : null}

      <ConfirmDialog
        isOpen={Boolean(retireAgent)}
        title={t("agents.retireTitle")}
        message={t("agents.retireMessage", { name: retireAgent?.name ?? "" })}
        confirmText={t("agents.retireConfirm")}
        pending={retireMutation.isPending}
        onConfirm={handleRetire}
        onCancel={() => setRetireAgentId(null)}
      />
    </div>
  );
}

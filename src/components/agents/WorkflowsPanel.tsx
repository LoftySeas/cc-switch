import { Ban, GitBranch, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  useCancelWorkflowRunMutation,
  useWorkflowOverviewsQuery,
} from "@/lib/query/agentOs";
import { extractErrorMessage } from "@/utils/errorUtils";

const terminal = new Set(["succeeded", "failed", "cancelled"]);

export function WorkflowsPanel() {
  const { t } = useTranslation();
  const query = useWorkflowOverviewsQuery();
  const cancel = useCancelWorkflowRunMutation();

  const cancelRun = async (runId: string, expectedRevision: number) => {
    try {
      await cancel.mutateAsync({ runId, expectedRevision });
      toast.success(t("agentOs.workflows.cancelled"));
    } catch (error) {
      toast.error(t("agentOs.operationFailed"), {
        description: extractErrorMessage(error),
      });
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col px-6">
      <p className="mb-4 text-sm text-muted-foreground">
        {t("agentOs.workflows.description")}
      </p>
      <ScrollArea className="min-h-0 flex-1">
        {query.isLoading ? (
          <p className="py-12 text-center text-sm text-muted-foreground">
            {t("agentOs.loading")}
          </p>
        ) : query.isError ? (
          <div className="py-12 text-center">
            <Button variant="outline" onClick={() => query.refetch()}>
              <RefreshCw className="mr-2 h-4 w-4" /> {t("agents.retry")}
            </Button>
          </div>
        ) : query.data?.length === 0 ? (
          <div className="py-12 text-center text-muted-foreground">
            <GitBranch className="mx-auto mb-3 h-8 w-8 opacity-60" />
            <p className="text-sm">{t("agentOs.workflows.empty")}</p>
            <p className="mt-1 text-xs">{t("agentOs.workflows.emptyHint")}</p>
          </div>
        ) : (
          <div className="space-y-4 pb-20">
            {query.data?.map(({ definition, runs }) => (
              <section
                key={`${definition.id}:${definition.version}`}
                className="rounded-xl border border-border-default p-4"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <h3 className="font-medium">{definition.name}</h3>
                    <p className="mt-1 text-sm text-muted-foreground">
                      {definition.purpose}
                    </p>
                  </div>
                  <Badge variant="outline">v{definition.version}</Badge>
                </div>
                <div className="mt-3 flex flex-wrap gap-3 text-xs text-muted-foreground">
                  <span className="font-mono">{definition.id}</span>
                  <span>
                    {t("agentOs.workflows.team", { id: definition.teamId })}
                  </span>
                  <span>
                    {t("agentOs.workflows.steps", {
                      count: definition.steps.length,
                    })}
                  </span>
                </div>
                <div className="mt-4 space-y-2">
                  {runs.length === 0 ? (
                    <p className="text-xs text-muted-foreground">
                      {t("agentOs.workflows.noRuns")}
                    </p>
                  ) : (
                    runs.map((run) => (
                      <div
                        key={run.id}
                        className="flex flex-wrap items-center gap-2 rounded-lg bg-muted/50 px-3 py-2"
                      >
                        <span className="min-w-0 flex-1 truncate font-mono text-xs">
                          {run.id}
                        </span>
                        <Badge variant="outline">
                          {t(`agentOs.states.${run.lifecycle}`)}
                        </Badge>
                        <span className="text-xs text-muted-foreground">
                          r{run.revision}
                        </span>
                        {!terminal.has(run.lifecycle) ? (
                          <Button
                            size="sm"
                            variant="ghost"
                            disabled={cancel.isPending}
                            onClick={() => void cancelRun(run.id, run.revision)}
                          >
                            <Ban className="mr-1 h-3.5 w-3.5" />{" "}
                            {t("agentOs.workflows.cancel")}
                          </Button>
                        ) : null}
                      </div>
                    ))
                  )}
                </div>
              </section>
            ))}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}

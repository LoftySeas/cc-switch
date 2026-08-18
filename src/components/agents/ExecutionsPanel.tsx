import { Activity, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useExecutionsQuery } from "@/lib/query/agentOs";

export function ExecutionsPanel() {
  const { t } = useTranslation();
  const query = useExecutionsQuery();
  const records = [...(query.data ?? [])].reverse();

  return (
    <div className="flex min-h-0 flex-1 flex-col px-6">
      <p className="mb-4 text-sm text-muted-foreground">
        {t("agentOs.executions.description")}
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
        ) : records.length === 0 ? (
          <div className="py-12 text-center text-muted-foreground">
            <Activity className="mx-auto mb-3 h-8 w-8 opacity-60" />
            <p className="text-sm">{t("agentOs.executions.empty")}</p>
          </div>
        ) : (
          <div className="space-y-3 pb-20">
            {records.map((record) => {
              return (
                <article
                  key={record.executionId}
                  className="rounded-xl border border-border-default p-4"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="min-w-0 flex-1 truncate font-mono text-sm">
                      {record.executionId}
                    </span>
                    <Badge variant="outline">
                      {t(`agentOs.states.${record.state}`)}
                    </Badge>
                    <span className="text-xs text-muted-foreground">
                      r{record.revision}
                    </span>
                  </div>
                  <p className="mt-2 text-sm">{record.objective}</p>
                  <div className="mt-3 grid gap-1 text-xs text-muted-foreground sm:grid-cols-2">
                    <span>
                      {t("agentOs.executions.agent", {
                        id: record.agentId,
                      })}
                    </span>
                    <span>
                      {t("agentOs.executions.runtime", {
                        id: record.runtimeId,
                      })}
                    </span>
                    <span>
                      {t("agentOs.executions.model", {
                        id: record.modelId,
                      })}
                    </span>
                    <span>
                      {t("agentOs.executions.transitions", {
                        count: record.transitionCount,
                      })}
                    </span>
                  </div>
                  <div className="mt-3">
                    <p className="mb-1 text-xs font-medium text-muted-foreground">
                      {t("agentOs.executions.contextReferences")}
                    </p>
                    {record.contextReferences.length === 0 ? (
                      <p className="text-xs text-muted-foreground">
                        {t("agentOs.executions.noContextReferences")}
                      </p>
                    ) : (
                      <div className="flex flex-wrap gap-1.5">
                        {record.contextReferences.map((reference, index) => (
                          <Badge
                            key={`${reference}-${index}`}
                            variant="secondary"
                          >
                            {reference}
                          </Badge>
                        ))}
                      </div>
                    )}
                  </div>
                  {record.resultSummary ? (
                    <p className="mt-3 rounded-lg bg-muted/50 px-3 py-2 text-xs">
                      {record.resultSummary}
                    </p>
                  ) : null}
                </article>
              );
            })}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}

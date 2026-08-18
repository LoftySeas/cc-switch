import { RefreshCw, Users } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useTeamsQuery } from "@/lib/query/agentOs";

export function TeamsPanel() {
  const { t } = useTranslation();
  const query = useTeamsQuery();
  return (
    <div className="flex min-h-0 flex-1 flex-col px-6">
      <p className="mb-4 text-sm text-muted-foreground">
        {t("agentOs.teams.description")}
      </p>
      <ScrollArea className="min-h-0 flex-1">
        {query.isLoading ? (
          <p className="py-12 text-center text-sm text-muted-foreground">
            {t("agentOs.loading")}
          </p>
        ) : query.isError ? (
          <div className="py-12 text-center">
            <Button variant="outline" onClick={() => query.refetch()}>
              <RefreshCw className="mr-2 h-4 w-4" />
              {t("agents.retry")}
            </Button>
          </div>
        ) : query.data?.length === 0 ? (
          <div className="py-12 text-center text-muted-foreground">
            <Users className="mx-auto mb-3 h-8 w-8 opacity-60" />
            <p className="text-sm">{t("agentOs.teams.empty")}</p>
          </div>
        ) : (
          <div className="space-y-3 pb-20">
            {query.data?.map((team) => (
              <article
                key={team.teamId}
                className="rounded-xl border border-border-default p-4"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <span className="min-w-0 flex-1 font-medium">
                    {team.name}
                  </span>
                  <Badge variant="outline">
                    {t(`agentOs.states.${team.lifecycle}`)}
                  </Badge>
                  <span className="text-xs text-muted-foreground">
                    r{team.revision}
                  </span>
                </div>
                <p className="mt-1 font-mono text-xs text-muted-foreground">
                  {team.teamId}
                </p>
                <p className="mt-2 text-sm">{team.purpose}</p>
                <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted-foreground">
                  <span>{t("agentOs.teams.owner", { id: team.ownerRef })}</span>
                  <span>
                    {t("agentOs.teams.members", {
                      count: team.memberships.length,
                    })}
                  </span>
                  <span>
                    {t("agentOs.teams.relationships", {
                      count: team.relationships.length,
                    })}
                  </span>
                </div>
                {team.memberships.length > 0 ? (
                  <div className="mt-3 flex flex-wrap gap-1.5">
                    {team.memberships.map((member) => (
                      <Badge key={member.membershipId} variant="secondary">
                        {member.label ?? member.agentId} ·{" "}
                        {t(`agentOs.states.${member.lifecycle}`)}
                      </Badge>
                    ))}
                  </div>
                ) : null}
              </article>
            ))}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}

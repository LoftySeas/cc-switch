import { Activity, Bot, GitBranch, Users } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AgentsPanel } from "./AgentsPanel";
import { ExecutionsPanel } from "./ExecutionsPanel";
import { WorkflowsPanel } from "./WorkflowsPanel";
import { TeamsPanel } from "./TeamsPanel";

interface AgentOsPanelProps {
  onOpenChange: (open: boolean) => void;
}

export function AgentOsPanel({ onOpenChange }: AgentOsPanelProps) {
  const { t } = useTranslation();
  return (
    <Tabs defaultValue="agents" className="flex min-h-0 flex-1 flex-col">
      <div className="px-6 pb-4">
        <TabsList>
          <TabsTrigger value="agents">
            <Bot className="mr-2 h-4 w-4" />
            {t("agentOs.tabs.agents")}
          </TabsTrigger>
          <TabsTrigger value="workflows">
            <GitBranch className="mr-2 h-4 w-4" />
            {t("agentOs.tabs.workflows")}
          </TabsTrigger>
          <TabsTrigger value="teams">
            <Users className="mr-2 h-4 w-4" />
            {t("agentOs.tabs.teams")}
          </TabsTrigger>
          <TabsTrigger value="executions">
            <Activity className="mr-2 h-4 w-4" />
            {t("agentOs.tabs.executions")}
          </TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="agents" className="m-0 min-h-0 flex-1">
        <AgentsPanel onOpenChange={onOpenChange} />
      </TabsContent>
      <TabsContent value="workflows" className="m-0 min-h-0 flex-1">
        <WorkflowsPanel />
      </TabsContent>
      <TabsContent value="teams" className="m-0 min-h-0 flex-1">
        <TeamsPanel />
      </TabsContent>
      <TabsContent value="executions" className="m-0 min-h-0 flex-1">
        <ExecutionsPanel />
      </TabsContent>
    </Tabs>
  );
}

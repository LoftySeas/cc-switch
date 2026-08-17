import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentOsPanel } from "@/components/agents/AgentOsPanel";

const mocks = vi.hoisted(() => ({
  listAgents: vi.fn(),
  listWorkflows: vi.fn(),
  listExecutions: vi.fn(),
}));

vi.mock("@/lib/api/agents", () => ({
  agentsApi: {
    list: mocks.listAgents,
    get: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    setLifecycle: vi.fn(),
  },
}));

vi.mock("@/lib/api/agentOs", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/lib/api/agentOs")>();
  return {
    ...original,
    agentOsApi: {
      ...original.agentOsApi,
      listWorkflows: mocks.listWorkflows,
      listWorkflowRuns: vi.fn().mockResolvedValue([]),
      listWorkflowTasks: vi.fn().mockResolvedValue([]),
      cancelWorkflowRun: vi.fn(),
      listExecutions: mocks.listExecutions,
      getExecution: vi.fn(),
    },
  };
});

describe("AgentOsPanel", () => {
  beforeEach(() => {
    mocks.listAgents.mockResolvedValue([]);
    mocks.listWorkflows.mockResolvedValue([]);
    mocks.listExecutions.mockResolvedValue([]);
  });

  it("exposes agents, governed workflows, and immutable execution history", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={queryClient}>
        <AgentOsPanel onOpenChange={vi.fn()} />
      </QueryClientProvider>,
    );

    expect(
      screen.getByRole("tab", { name: "agentOs.tabs.agents" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("tab", { name: "agentOs.tabs.workflows" }),
    );
    expect(
      await screen.findByText("agentOs.workflows.empty"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("tab", { name: "agentOs.tabs.executions" }),
    );
    expect(
      await screen.findByText("agentOs.executions.empty"),
    ).toBeInTheDocument();
  });
});

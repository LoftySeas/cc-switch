import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsPanel } from "@/components/agents/AgentsPanel";
import type { Agent } from "@/lib/api/agents";

const mocks = vi.hoisted(() => ({
  agents: [] as Agent[],
  list: vi.fn(),
  get: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  setLifecycle: vi.fn(),
  toastSuccess: vi.fn(),
  toastWarning: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("@/lib/api/agents", () => ({
  agentsApi: {
    list: mocks.list,
    get: mocks.get,
    create: mocks.create,
    update: mocks.update,
    setLifecycle: mocks.setLifecycle,
  },
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    warning: mocks.toastWarning,
    error: mocks.toastError,
  },
}));

const makeAgent = (overrides: Partial<Agent> = {}): Agent => ({
  id: "agent-1",
  name: "Researcher",
  description: "Reviews source material",
  owner: "Architecture",
  lifecycleState: "active",
  revision: 3,
  createdAt: 1,
  updatedAt: 2,
  ...overrides,
});

const renderPanel = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <AgentsPanel onOpenChange={vi.fn()} />
    </QueryClientProvider>,
  );
};

describe("AgentsPanel", () => {
  beforeEach(() => {
    mocks.agents = [
      makeAgent(),
      makeAgent({
        id: "agent-2",
        name: "Archivist",
        owner: "Records",
        lifecycleState: "retired",
        revision: 7,
      }),
    ];
    mocks.list.mockImplementation(async () => [...mocks.agents]);
    mocks.get.mockImplementation(async (id: string) =>
      mocks.agents.find((agent) => agent.id === id),
    );
    mocks.create.mockReset();
    mocks.update.mockReset();
    mocks.setLifecycle.mockReset();
  });

  it("lists and searches stable identities while keeping retired agents read-only", async () => {
    renderPanel();

    expect(await screen.findByText("Researcher")).toBeInTheDocument();
    expect(screen.getByText("Archivist")).toBeInTheDocument();
    expect(screen.getByText("agents.retiredReadOnly")).toBeInTheDocument();
    expect(screen.getAllByLabelText("agents.editAgent")).toHaveLength(1);
    expect(screen.getAllByLabelText("agents.retireAgent")).toHaveLength(1);

    fireEvent.change(screen.getByLabelText("agents.searchAriaLabel"), {
      target: { value: "records" },
    });

    expect(screen.queryByText("Researcher")).not.toBeInTheDocument();
    expect(screen.getByText("Archivist")).toBeInTheDocument();
  });

  it("creates an agent with identity metadata", async () => {
    const created = makeAgent({
      id: "agent-3",
      name: "Planner",
      owner: "Product",
      description: "Plans milestones",
      lifecycleState: "draft",
      revision: 1,
    });
    mocks.create.mockResolvedValue(created);
    const user = userEvent.setup();
    renderPanel();

    await screen.findByText("Researcher");
    await user.click(screen.getByRole("button", { name: "agents.create" }));
    await user.type(screen.getByLabelText("agents.name"), "Planner");
    await user.type(screen.getByLabelText("agents.owner"), "Product");
    await user.type(
      screen.getByLabelText("agents.description"),
      "Plans milestones",
    );
    await user.click(screen.getByRole("button", { name: "agents.create" }));

    await waitFor(() =>
      expect(mocks.create).toHaveBeenCalledWith({
        name: "Planner",
        owner: "Product",
        description: "Plans milestones",
      }),
    );
    expect(await screen.findByText("Planner")).toBeInTheDocument();
  });

  it("updates metadata and retires an agent with its current revision", async () => {
    const updated = makeAgent({ owner: "Platform", revision: 4 });
    mocks.update.mockResolvedValue(updated);
    mocks.setLifecycle.mockResolvedValue(
      makeAgent({ lifecycleState: "retired", revision: 5 }),
    );
    const user = userEvent.setup();
    renderPanel();

    await screen.findByText("Researcher");
    await user.click(screen.getByLabelText("agents.editAgent"));
    const owner = screen.getByLabelText("agents.owner");
    await user.clear(owner);
    await user.type(owner, "Platform");
    await user.click(screen.getByRole("button", { name: "agents.save" }));

    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith("agent-1", {
        name: "Researcher",
        description: "Reviews source material",
        owner: "Platform",
        expectedRevision: 3,
      }),
    );

    await user.click(screen.getByLabelText("agents.retireAgent"));
    await user.click(
      screen.getByRole("button", { name: "agents.retireConfirm" }),
    );

    await waitFor(() =>
      expect(mocks.setLifecycle).toHaveBeenCalledWith("agent-1", "retired", 4),
    );
  });

  it("preserves a draft on revision conflict and explicitly reloads the latest revision", async () => {
    mocks.update
      .mockRejectedValueOnce(new Error("Agent revision conflict"))
      .mockImplementationOnce(async (_id, input) =>
        makeAgent({
          name: input.name,
          owner: input.owner,
          description: input.description,
          revision: 5,
        }),
      );
    const user = userEvent.setup();
    renderPanel();

    await screen.findByText("Researcher");
    await user.click(screen.getByLabelText("agents.editAgent"));
    const name = screen.getByLabelText("agents.name");
    await user.clear(name);
    await user.type(name, "My draft");

    mocks.agents = [
      makeAgent({ name: "Remote edit", owner: "Remote", revision: 4 }),
    ];
    await user.click(screen.getByRole("button", { name: "agents.save" }));

    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByLabelText("agents.name")).toHaveValue("My draft");

    await user.click(
      screen.getByRole("button", { name: "agents.reloadLatest" }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("agents.name")).toHaveValue("Remote edit"),
    );

    await user.clear(screen.getByLabelText("agents.name"));
    await user.type(screen.getByLabelText("agents.name"), "Merged edit");
    await user.click(screen.getByRole("button", { name: "agents.save" }));

    await waitFor(() =>
      expect(mocks.update).toHaveBeenLastCalledWith(
        "agent-1",
        expect.objectContaining({
          name: "Merged edit",
          expectedRevision: 4,
        }),
      ),
    );
  });
});

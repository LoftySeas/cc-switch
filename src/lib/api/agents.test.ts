import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { agentsApi } from "./agents";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("agentsApi", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("uses the Agent OS command boundary without provider or model bindings", async () => {
    vi.mocked(invoke).mockResolvedValue({});

    await agentsApi.create({
      name: "Architect",
      description: "Owns boundaries",
      owner: "local-user",
    });
    await agentsApi.setLifecycle("agent-1", "active", 1);
    await agentsApi.update("agent-1", {
      expectedRevision: 2,
      description: "Updated safely",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "create_agent", {
      input: {
        name: "Architect",
        description: "Owns boundaries",
        owner: "local-user",
      },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "set_agent_lifecycle", {
      id: "agent-1",
      lifecycleState: "active",
      expectedRevision: 1,
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "update_agent", {
      id: "agent-1",
      input: {
        expectedRevision: 2,
        description: "Updated safely",
      },
    });
  });
});

import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModelRoutingPanel } from "@/components/proxy/ModelRoutingPanel";
import { modelRoutingApi } from "@/lib/api/modelRouting";

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getAll: vi.fn().mockResolvedValue({
      gpt: { id: "gpt", name: "GPT relay", settingsConfig: {} },
      grok: { id: "grok", name: "Grok relay", settingsConfig: {} },
    }),
  },
}));
vi.mock("@/utils/providerCapabilities", () => ({
  resolveCodexOfficialIdentity: () => null,
}));
vi.mock("@/lib/api/modelRouting", () => ({
  modelRoutingApi: {
    get: vi.fn(),
    save: vi.fn(),
    preview: vi.fn(),
  },
}));

describe("ModelRoutingPanel", () => {
  beforeEach(() => {
    vi.mocked(modelRoutingApi.get).mockResolvedValue({
      version: 1,
      enabled: true,
      rules: [
        {
          id: "1",
          enabled: true,
          matchType: "prefix",
          pattern: "gpt-",
          providerId: "gpt",
        },
        {
          id: "2",
          enabled: true,
          matchType: "prefix",
          pattern: "grok-",
          providerId: "grok",
        },
      ],
    });
    vi.mocked(modelRoutingApi.save).mockReset();
  });
  it("edits, reorders and deletes rules before saving", async () => {
    render(<ModelRoutingPanel />);
    await screen.findByDisplayValue("gpt-");
    fireEvent.click(screen.getByLabelText("上移规则 2"));
    expect(screen.getByLabelText("模型匹配值 1")).toHaveValue("grok-");
    fireEvent.change(screen.getByLabelText("模型匹配值 1"), {
      target: { value: "grok-4" },
    });
    fireEvent.click(screen.getAllByText("删除")[1]);
    vi.mocked(modelRoutingApi.save).mockResolvedValue({
      restartRequired: false,
    });
    fireEvent.click(screen.getByText("保存规则"));
    await waitFor(() =>
      expect(modelRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          rules: [
            expect.objectContaining({ pattern: "grok-4", providerId: "grok" }),
          ],
        }),
      ),
    );
  });
  it("preserves draft on save failure and reports catalog restart only when needed", async () => {
    render(<ModelRoutingPanel />);
    await screen.findByDisplayValue("gpt-");
    vi.mocked(modelRoutingApi.save).mockRejectedValueOnce("disk unavailable");
    fireEvent.click(screen.getByText("保存规则"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "disk unavailable",
    );
    expect(screen.getByDisplayValue("grok-")).toBeInTheDocument();
    vi.mocked(modelRoutingApi.save).mockResolvedValueOnce({
      restartRequired: true,
    });
    fireEvent.click(screen.getByText("保存规则"));
    expect(await screen.findByRole("status")).toHaveTextContent("请重启 Codex");
  });
  it("previews the selected target and clears stale preview on edits", async () => {
    render(<ModelRoutingPanel />);
    await screen.findByDisplayValue("gpt-");
    vi.mocked(modelRoutingApi.preview).mockResolvedValue({
      matchedRuleId: "2",
      providerId: "grok",
      catalog: {
        models: [{ slug: "grok-test", display_name: "Grok" }],
        warnings: [],
      },
    });
    fireEvent.change(screen.getByLabelText("预览模型"), {
      target: { value: "grok-test" },
    });
    fireEvent.click(screen.getByText("预览路由与目录"));
    expect(
      await screen.findByText("命中规则 → Grok relay"),
    ).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("模型匹配值 2"), {
      target: { value: "other-" },
    });
    expect(screen.queryByText("命中规则 → Grok relay")).not.toBeInTheDocument();
  });
});

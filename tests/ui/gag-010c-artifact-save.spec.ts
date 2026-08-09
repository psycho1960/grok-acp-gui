import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { DesktopCommand, TaskId } from "../../src/bridge/types";
import ArtifactPanel from "../../src/features/conversation/ArtifactPanel.vue";

const pickArtifactSavePath = vi.hoisted(() => vi.fn());

vi.mock("../../src/bridge/artifact-save-picker", () => ({
  pickArtifactSavePath,
}));

const taskId = "task-gag-010c" as TaskId;
const readyArtifact = {
  artifactId: "artifact-ready",
  displayName: "结果 图片.png",
  mimeType: "image/png",
  bytes: 128,
  state: "ready",
  previewCapability: "inline",
} as const;

function listResult() {
  return { success: "true" as const, data: { artifacts: [readyArtifact] } };
}

beforeEach(() => {
  pickArtifactSavePath.mockReset();
});

describe("GAG-010C Artifact Panel save as", () => {
  it("treats native dialog cancellation as a structured no-op", async () => {
    pickArtifactSavePath.mockResolvedValue({
      status: "cancelled",
      artifactId: readyArtifact.artifactId,
      message: "已取消另存为，未修改任何文件",
    });
    const commands: DesktopCommand[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        commands.push(command);
        if (command.type === "artifact.list") return listResult();
        throw new Error(`unexpected command ${command.type}`);
      },
    });
    const wrapper = mount(ArtifactPanel, {
      props: { bridge, taskId, refreshKey: 0 },
    });
    await flushPromises();
    await wrapper.get('[data-testid="save-artifact"]').trigger("click");
    await flushPromises();

    expect(wrapper.get('[data-testid="artifact-save-notice"]').text()).toContain("已取消另存为");
    expect(commands.some((command) => command.type === "artifact.save")).toBe(false);
    wrapper.unmount();
  });

  it("shows all conflict choices and overwrites only after explicit confirmation", async () => {
    const targetPath = "C:\\Users\\测试 用户\\结果 图片.png";
    pickArtifactSavePath.mockResolvedValue({ status: "selected", path: targetPath });
    const commands: DesktopCommand[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        commands.push(command);
        if (command.type === "artifact.list") return listResult();
        if (command.type === "artifact.save" && !command.payload.overwrite) {
          return {
            success: "true",
            data: {
              status: "conflict",
              artifactId: readyArtifact.artifactId,
              targetName: readyArtifact.displayName,
              message: "目标文件已存在",
            },
          };
        }
        if (command.type === "artifact.save" && command.payload.overwrite) {
          return {
            success: "true",
            data: {
              status: "saved",
              artifactId: readyArtifact.artifactId,
              targetName: readyArtifact.displayName,
              message: "制品已安全保存",
            },
          };
        }
        if (command.type === "artifact.reveal") {
          return { success: "true", data: { revealed: true } };
        }
        throw new Error(`unexpected command ${command.type}`);
      },
    });
    const wrapper = mount(ArtifactPanel, {
      props: { bridge, taskId, refreshKey: 0 },
    });
    await flushPromises();

    await wrapper.get('[data-testid="save-artifact"]').trigger("click");
    await flushPromises();
    const conflict = wrapper.get('[data-testid="artifact-save-conflict"]');
    expect(conflict.get('[data-testid="cancel-overwrite"]').exists()).toBe(true);
    expect(conflict.get('[data-testid="rename-artifact"]').exists()).toBe(true);
    expect(conflict.get('[data-testid="confirm-overwrite"]').exists()).toBe(true);

    const firstSave = commands.find((command) => command.type === "artifact.save");
    expect(firstSave).toEqual({
      type: "artifact.save",
      payload: {
        taskId,
        artifactId: readyArtifact.artifactId,
        targetPath,
        overwrite: false,
      },
    });
    expect(JSON.stringify(firstSave)).not.toMatch(/base64|cachePath|binary|sourcePath/i);

    await conflict.get('[data-testid="confirm-overwrite"]').trigger("click");
    await flushPromises();
    const saves = commands.filter((command) => command.type === "artifact.save");
    expect(saves).toHaveLength(2);
    expect(saves[1]).toMatchObject({ payload: { overwrite: true } });
    expect(wrapper.get('[data-testid="artifact-save-notice"]').text()).toContain("已保存");

    await wrapper.get('[data-testid="reveal-saved-artifact"]').trigger("click");
    await flushPromises();
    expect(commands.at(-1)).toEqual({
      type: "artifact.reveal",
      payload: { taskId, artifactId: readyArtifact.artifactId, targetPath },
    });
    wrapper.unmount();
  });

  it("keeps missing, quarantined, and rejected artifacts unsaveable", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "artifact.list") {
          return {
            success: "true",
            data: {
              artifacts: ["missing", "quarantined", "rejected"].map((state) => ({
                ...readyArtifact,
                artifactId: `artifact-${state}`,
                state,
              })),
            },
          };
        }
        throw new Error(`unexpected command ${command.type}`);
      },
    });
    const wrapper = mount(ArtifactPanel, {
      props: { bridge, taskId, refreshKey: 0 },
    });
    await flushPromises();
    const buttons = wrapper.findAll('[data-testid="save-artifact"]');
    expect(buttons).toHaveLength(3);
    expect(buttons.every((button) => button.attributes("disabled") !== undefined)).toBe(true);
    expect(pickArtifactSavePath).not.toHaveBeenCalled();
    wrapper.unmount();
  });
});

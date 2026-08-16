import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type {
  ArtifactDescriptor,
  DesktopCommand,
  DesktopResult,
  WorktreeRecord,
} from "../../src/bridge/types";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import ShellView from "../../src/app/ShellView.vue";
import {
  FIX_TASK,
  fixtureArtifact,
  fixtureSessionSnapshot,
} from "../../src/features/conversation/fixtures";
import type { SessionTimelineSnapshot, TimelineItem } from "../../src/features/conversation/types";

function itemBase(kind: TimelineItem["kind"], seq: number, extra: object): TimelineItem {
  return {
    id: `${kind}-${seq}`,
    kind,
    seq,
    sessionId: "sess-conv-1",
    timestamp: "2026-08-05T12:00:00.000Z",
    eventKey: `sess-conv-1:${seq}`,
    ...extra,
  } as TimelineItem;
}

async function mountConversation(
  snapshot: Partial<SessionTimelineSnapshot> = {},
  bridge = createFakeDesktopBridge(),
) {
  const w = mount(ConversationView, {
    props: {
      bridge,
      taskId: FIX_TASK,
      snapshot: fixtureSessionSnapshot(snapshot),
    },
    attachTo: document.body,
  });
  await flushPromises();
  return w;
}

describe("GAG-021 conversation rail", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("stays closed when there are no artifacts and the workspace is a healthy isolation", async () => {
    const w = await mountConversation({
      workspaceStrategy: "worktree",
      workspaceAvailable: true,
      events: [],
      items: [
        itemBase("user", 1, { text: "继续隔离工作" }),
        itemBase("assistant", 2, { text: "好的。", streaming: false, frozen: true }),
      ],
    });
    expect(w.find('[data-testid="conversation-rail"]').exists()).toBe(false);
    expect(w.find('[data-testid="artifact-panel"]').exists()).toBe(false);
    w.unmount();
  });

  it("opens for listed artifacts and keeps gallery actions in the rail", async () => {
    const artifact: ArtifactDescriptor = {
      artifactId: "art-1",
      displayName: "screenshot.png",
      mimeType: "image/png",
      bytes: 2048,
      state: "ready",
      previewCapability: "inline",
    };
    const w = await mountConversation(
      {
        workspaceStrategy: "worktree",
        workspaceAvailable: true,
        events: [fixtureArtifact(1)],
      },
      createFakeDesktopBridge({
        onExecute(command) {
          if (command.type === "artifact.list") {
            return { success: "true", data: { artifacts: [artifact] } };
          }
          return { success: "true", data: { acknowledged: command.type } };
        },
      }),
    );
    expect(w.get('[data-testid="conversation-rail"]').exists()).toBe(true);
    expect(w.get('[data-testid="artifact-chip"]').text()).toContain("screenshot.png");
    expect(w.get('[data-testid="artifact-chip"]').text()).not.toMatch(/预览|另存为|显示位置|在右侧查看/);
    const rail = w.get('[data-testid="conversation-rail"]');
    expect(rail.get('[data-testid="artifact-gallery"]').exists()).toBe(true);
    expect(rail.get('[data-testid="save-artifact"]').exists()).toBe(true);
    expect(rail.text()).toMatch(/预览|加载预览/);
    expect(rail.text()).toContain("显示位置");
    expect(w.get('[data-testid="rail-tab-artifacts"]').attributes("aria-selected")).toBe("true");
    w.unmount();
  });

  it("shows direct workspace information without invoking Worktree operations", async () => {
    const commands: DesktopCommand[] = [];
    const artifact: ArtifactDescriptor = {
      artifactId: "art-direct",
      displayName: "result.png",
      mimeType: "image/png",
      bytes: 1024,
      state: "ready",
      previewCapability: "inline",
    };
    const w = await mountConversation(
      {
        workspaceStrategy: "direct",
        workspaceAvailable: true,
        events: [fixtureArtifact(1)],
      },
      createFakeDesktopBridge({
        onExecute(command) {
          commands.push(command);
          if (command.type === "artifact.list") {
            return { success: "true", data: { artifacts: [artifact] } };
          }
          if (command.type === "worktree.inspect") {
            return {
              success: "false",
              error: { code: "WORKTREE_NOT_READY", message: "Worktree is not registered" },
            };
          }
          return { success: "true", data: { acknowledged: command.type } };
        },
      }),
    );

    await w.get('[data-testid="rail-tab-workspace"]').trigger("click");
    await flushPromises();

    const rail = w.get('[data-testid="conversation-rail"]');
    expect(rail.text()).toContain("当前项目目录");
    expect(rail.text()).not.toContain("Worktree is not registered");
    expect(commands.some((command) => command.type === "worktree.inspect")).toBe(false);
    w.unmount();
  });

  it("opens for not-created, conflicted, external-awaiting-adoption, and cleanup-recovery-pending workspaces", async () => {
    const notCreatedCommands: DesktopCommand[] = [];
    const notCreated = await mountConversation(
      {
        workspaceStrategy: "worktree",
        workspaceAvailable: false,
        events: [],
        items: [itemBase("user", 1, { text: "还没建好" })],
      },
      createFakeDesktopBridge({
        onExecute(command) {
          notCreatedCommands.push(command);
          return { success: "true", data: { acknowledged: command.type } };
        },
      }),
    );
    expect(notCreated.get('[data-testid="conversation-rail"]').exists()).toBe(true);
    expect(notCreated.get('[data-testid="rail-tab-workspace"]').attributes("aria-selected")).toBe(
      "true",
    );
    expect(notCreated.get('[data-testid="worktree-not-created"]').text()).toContain(
      "隔离 Worktree 尚未创建",
    );
    expect(notCreated.text()).not.toContain("Worktree is not registered");
    expect(
      notCreatedCommands.some((command) => command.type === "worktree.inspect"),
    ).toBe(false);
    notCreated.unmount();

    const conflicted = await mountConversation({
      workspaceStrategy: "worktree",
      workspaceAvailable: true,
      taskStatus: "conflicted",
      events: [],
      items: [itemBase("user", 1, { text: "有冲突" })],
    });
    expect(conflicted.get('[data-testid="conversation-rail"]').exists()).toBe(true);
    conflicted.unmount();

    function worktree(overrides: Partial<WorktreeRecord> = {}): WorktreeRecord {
      return {
        id: "wt-1",
        taskId: FIX_TASK,
        repoRoot: "D:\\repo",
        path: "D:\\repo\\.worktrees\\task",
        displayPath: "repo/task",
        branch: "feat/task",
        baseBranch: "main",
        baseCommit: "abc123",
        ownership: "managed",
        state: "ready",
        ...overrides,
      };
    }

    const external = await mountConversation(
      {
        workspaceStrategy: "worktree",
        workspaceAvailable: true,
        events: [],
        items: [itemBase("user", 1, { text: "外部" })],
      },
      createFakeDesktopBridge({
        onExecute(command) {
          if (command.type === "worktree.inspect") {
            return {
              success: "true",
              data: { worktree: worktree({ ownership: "external" }) },
            };
          }
          return { success: "true", data: { acknowledged: command.type } };
        },
      }),
    );
    expect(external.get('[data-testid="conversation-rail"]').exists()).toBe(true);
    external.unmount();

    const cleanup = await mountConversation(
      {
        workspaceStrategy: "worktree",
        workspaceAvailable: true,
        events: [],
        items: [itemBase("user", 1, { text: "待清理" })],
      },
      createFakeDesktopBridge({
        onExecute(command) {
          if (command.type === "worktree.inspect") {
            return {
              success: "true",
              data: { worktree: worktree({ state: "quarantined" }) },
            };
          }
          return { success: "true", data: { acknowledged: command.type } };
        },
      }),
    );
    expect(cleanup.get('[data-testid="conversation-rail"]').exists()).toBe(true);
    cleanup.unmount();
  });

  it("opens the rail on the clicked user-bubble thumbnail", async () => {
    const commands: DesktopCommand[] = [];
    const image: ArtifactDescriptor = {
      artifactId: "art-img-1",
      displayName: "shot.png",
      mimeType: "image/png",
      bytes: 2048,
      state: "ready",
      previewCapability: "inline",
    };
    const w = await mountConversation(
      {
        workspaceStrategy: "worktree",
        workspaceAvailable: true,
        events: [],
        items: [
          itemBase("user", 1, {
            text: "看这张图",
            attachments: [image],
          }),
        ],
      },
      createFakeDesktopBridge({
        onExecute(command): DesktopResult {
          commands.push(command);
          if (command.type === "artifact.list") {
            return { success: "true", data: { artifacts: [] } };
          }
          if (command.type === "artifact.preview") {
            return {
              success: "true",
              data: { artifact: image, url: "app://preview/art-img-1" },
            };
          }
          return { success: "true", data: { acknowledged: command.type } };
        },
      }),
    );
    expect(w.find('[data-testid="conversation-rail"]').exists()).toBe(false);
    await w.get('[data-testid="user-thumb"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="conversation-rail"]').exists()).toBe(true);
    expect(w.get('[data-testid="rail-tab-artifacts"]').attributes("aria-selected")).toBe("true");
    expect(commands.some((command) => command.type === "artifact.preview" && command.payload.artifactId === "art-img-1")).toBe(true);
    w.unmount();
  });

  it("does not add a second right column while conversation is open", async () => {
    window.location.hash = `#conversation/${FIX_TASK}`;
    const w = mount(ShellView, {
      global: { plugins: [createPinia()] },
      attachTo: document.body,
    });
    await flushPromises();
    expect(w.find('[data-testid="conversation-view"]').exists()).toBe(true);
    expect(w.find('[aria-label="检查器"]').exists()).toBe(false);
    expect(w.find('[data-testid="open-inspector"]').exists()).toBe(false);
    expect(w.findAll('[data-testid="conversation-rail"]').length).toBeLessThanOrEqual(1);
    w.unmount();
  });
});


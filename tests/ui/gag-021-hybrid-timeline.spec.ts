import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge, fakeError } from "../../src/bridge/fake-bridge";
import type { DesktopCommand } from "../../src/bridge/types";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import {
  FIX_SESSION,
  FIX_TASK,
  fixtureActivity,
  fixtureAssistantDelta,
  fixtureChanges,
  fixtureSessionSnapshot,
  fixtureTaskState,
  fixtureToolDelta,
} from "../../src/features/conversation/fixtures";
import type { SessionTimelineSnapshot, TimelineItem } from "../../src/features/conversation/types";

function itemBase(kind: TimelineItem["kind"], seq: number, extra: object): TimelineItem {
  return {
    id: `${kind}-${seq}`,
    kind,
    seq,
    sessionId: FIX_SESSION,
    timestamp: "2026-08-05T12:00:00.000Z",
    eventKey: `${FIX_SESSION}:${seq}`,
    ...extra,
  } as TimelineItem;
}

async function mountConversation(
  snapshot: Partial<SessionTimelineSnapshot> = {},
  onExecute?: (command: DesktopCommand) => unknown,
) {
  const w = mount(ConversationView, {
    props: {
      bridge: createFakeDesktopBridge({
        onExecute(command) {
          const extra = onExecute?.(command);
          if (extra) return extra;
          return { success: "true", data: { acknowledged: command.type } };
        },
      }),
      taskId: FIX_TASK,
      snapshot: fixtureSessionSnapshot(snapshot),
    },
    attachTo: document.body,
  });
  await flushPromises();
  return w;
}

describe("GAG-021 hybrid timeline", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("empty conversation asks the developer to send a goal", async () => {
    const w = await mountConversation({ cursor: 0, events: [], items: [], status: "idle" });
    expect(w.text()).toContain("把目标发给智能体");
    expect(w.text()).toContain("下方输入；需要时用 / 看快捷指令，或点回形针加图");
    expect(w.text()).not.toContain("还没有消息");
    w.unmount();
  });

  it("does not show event sequence or user/assistant kind badges", async () => {
    const w = await mountConversation({
      cursor: 2,
      events: [],
      items: [
        itemBase("user", 1, { text: "请检查模块" }),
        itemBase("assistant", 2, { text: "好的，我来看。", streaming: false, frozen: true }),
      ],
    });
    expect(w.get('[data-testid="user-message"]').text()).toContain("请检查模块");
    expect(w.get('[data-testid="assistant-message"]').text()).toContain("好的，我来看。");
    const view = w.get('[data-testid="conversation-view"]');
    expect(view.text()).not.toMatch(/#\d+/);
    expect(view.text()).not.toContain("用户");
    expect(view.text()).not.toContain("助手");
    w.unmount();
  });

  it("puts the user bubble on the right and keeps assistant prose on the left", async () => {
    const w = await mountConversation({
      cursor: 2,
      events: [],
      items: [
        itemBase("user", 1, { text: "请检查模块" }),
        itemBase("assistant", 2, { text: "好的，我来看。", streaming: false, frozen: true }),
      ],
    });
    expect(w.get('[data-testid="timeline-item-user"]').attributes("data-align")).toBe("end");
    expect(w.get('[data-testid="timeline-item-assistant"]').attributes("data-align")).toBe("start");
    expect(w.get('[data-testid="assistant-message"]').attributes("data-chrome")).toBe("prose");
    w.unmount();
  });

  it("shows sent images as 72 by 72 thumbnails inside the user bubble", async () => {
    const w = await mountConversation(
      {
        cursor: 1,
        events: [],
        items: [
          itemBase("user", 1, {
            text: "看这张图",
            attachments: [
              {
                artifactId: "art-img-1",
                displayName: "shot.png",
                mimeType: "image/png",
                bytes: 2048,
                state: "ready",
                previewCapability: "inline",
              },
            ],
          }),
        ],
      },
      (command) => {
        if (command.type === "artifact.preview") {
          return {
            success: "true",
            data: {
              artifact: {
                artifactId: "art-img-1",
                displayName: "shot.png",
                mimeType: "image/png",
                bytes: 2048,
                state: "ready",
                previewCapability: "inline",
              },
              url: "asset://preview/art-img-1",
            },
          };
        }
        return undefined;
      },
    );
    const thumb = w.get('[data-testid="user-thumb"]');
    expect(thumb.attributes("width")).toBe("72");
    expect(thumb.attributes("height")).toBe("72");
    expect(thumb.attributes("src")).toBe("asset://preview/art-img-1");
    expect(w.get('[data-testid="user-message"]').text()).toContain("看这张图");
    w.unmount();
  });

  it("shows a placeholder and explanation when the image cache is missing", async () => {
    const w = await mountConversation({
      cursor: 1,
      events: [],
      items: [
        itemBase("user", 1, {
          text: "缺图",
          attachments: [
            {
              artifactId: "art-missing",
              displayName: "gone.png",
              mimeType: "image/png",
              bytes: 1024,
              state: "missing",
              previewCapability: "inline",
            },
          ],
        }),
      ],
    });
    expect(w.find('[data-testid="user-thumb"]').exists()).toBe(false);
    const placeholder = w.get('[data-testid="user-thumb-missing"]');
    expect(placeholder.text()).toMatch(/缓存|找不到|不可用/);
    expect(w.get('[data-testid="user-message"]').text()).toContain("gone.png");
    w.unmount();
  });

  it("keeps a pending user bubble the same shape at reduced opacity with 发送中", async () => {
    const w = await mountConversation({
      cursor: 1,
      events: [],
      items: [itemBase("user", 1, { text: "正在发出", pending: true })],
    });
    const bubble = w.get('[data-testid="user-message"]');
    expect(bubble.text()).toContain("正在发出");
    expect(bubble.text()).toContain("发送中");
    expect(bubble.text()).not.toContain("发送中…");
    expect(bubble.attributes("data-pending")).toBe("true");
    expect(bubble.attributes("style") ?? "").toMatch(/opacity/);
    w.unmount();
  });

  it("keeps a visible processing status while a running turn has no safe work detail yet", async () => {
    const w = await mountConversation({
      cursor: 1,
      events: [],
      status: "running",
      items: [itemBase("user", 1, { text: "请开始处理" })],
    });

    const processing = w.get('[data-testid="agent-processing"]');
    expect(processing.text()).toContain("正在处理");
    expect(processing.classes()).toContain("surface-card");
    expect(processing.attributes("role")).toBe("status");
    expect(processing.attributes("aria-live")).toBe("polite");
    w.unmount();
  });

  it("stops showing a reply when a stale running snapshot contains a terminal event", async () => {
    const w = await mountConversation({
      status: "running",
      cursor: 3,
      events: [
        fixtureTaskState(1, "running"),
        fixtureToolDelta(2, {
          toolCallId: "tc-plan-stopped",
          title: "写入计划",
          kind: "edit",
          status: "running",
        }),
        fixtureTaskState(3, "interrupted", {
          reason: "session disconnected",
        }),
      ],
    });

    expect(w.find('[data-testid="agent-processing"]').exists()).toBe(false);
    expect(w.text()).not.toContain("Agent 正在回复");
    expect(w.get('[data-testid="resume-session"]').text()).toContain("恢复会话");
    w.unmount();
  });

  it("localizes an authentication failure and keeps recovery beside the latest error", async () => {
    const commands: DesktopCommand[] = [];
    const w = await mountConversation(
      {
        status: "error",
        cursor: 2,
        events: [],
        items: [
          itemBase("user", 1, { text: "回归测试" }),
          itemBase("error", 2, {
            code: "GROK_AUTH_REQUIRED",
            message:
              "[GROK_AUTH_REQUIRED] Grok authentication is required. Run 'grok login', then retry.",
            retryable: true,
          }),
        ],
      },
      (command) => {
        commands.push(command);
        return { success: "true", data: { acknowledged: command.type } };
      },
    );

    const error = w.get('[data-testid="error-item"]');
    expect(error.text()).toContain("Grok 未登录");
    expect(error.text()).toContain("grok login");
    expect(w.findAll('[data-testid="resume-session"]')).toHaveLength(1);
    expect(w.get('[data-testid="conversation-header"]').text()).not.toContain("恢复会话");

    await error.get('[data-testid="resume-session"]').trigger("click");
    await flushPromises();
    expect(commands).toContainEqual(expect.objectContaining({ type: "session.resume" }));
    w.unmount();
  });

  it("labels thinking in Chinese and expands only the ACP summary", async () => {
    const w = await mountConversation({
      cursor: 1,
      events: [],
      status: "running",
      items: [
        itemBase("thinking", 1, {
          summary: "正在规划方案",
          durationMs: undefined,
          expanded: false,
        }),
      ],
    });
    const toggle = w.get('[data-testid="thinking-toggle"]');
    expect(w.get('[data-testid="thinking-card"]').classes()).not.toContain("surface-card");
    expect(toggle.text()).toContain("思考中");
    expect(toggle.text()).not.toContain("Thinking");
    expect(toggle.text()).not.toContain("正在规划方案");
    expect(w.find('[data-testid="agent-processing"]').exists()).toBe(false);
    expect(w.find('[data-testid="thinking-body"]').exists()).toBe(false);
    await toggle.trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="thinking-body"]').text()).toBe("正在规划方案");
    w.unmount();
  });

  it("merges consecutive Grok Build thought chunks into one expandable work card", async () => {
    const w = await mountConversation({
      cursor: 3,
      items: undefined,
      events: [
        fixtureActivity(1, "thinking", "先检查工作区。"),
        fixtureActivity(2, "thinking", "然后读取项目说明。"),
        fixtureAssistantDelta(3, "我已经确认项目结构。"),
      ],
    });

    expect(w.findAll('[data-testid="thinking-card"]')).toHaveLength(1);
    const toggle = w.get('[data-testid="thinking-toggle"]');
    expect(toggle.text()).toContain("已思考");
    expect(toggle.text()).not.toContain("先检查工作区");
    expect(toggle.text()).not.toContain("然后读取项目说明");
    await toggle.trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="thinking-body"]').text()).toBe(
      "先检查工作区。然后读取项目说明。",
    );
    w.unmount();
  });

  it("collapses interleaved thinking and tools into one process activity row", async () => {
    const w = await mountConversation({
      cursor: 6,
      items: undefined,
      events: [
        fixtureActivity(1, "thinking", "先查看项目。"),
        fixtureToolDelta(2, {
          toolCallId: "read-1",
          title: "read_file",
          kind: "read",
          status: "completed",
          locations: ["D:\\codex\\grok acp gui\\src\\App.vue"],
        }),
        fixtureActivity(3, "thinking", "再运行测试。"),
        fixtureToolDelta(4, {
          toolCallId: "exec-1",
          title: "npm test",
          kind: "execute",
          status: "completed",
          durationMs: 1200,
        }),
        fixtureActivity(5, "thinking", "整理结论。"),
        fixtureAssistantDelta(6, "检查完成。"),
      ],
    });

    expect(w.findAll('[data-testid="process-activity"]')).toHaveLength(1);
    const process = w.get('[data-testid="process-activity"]');
    expect(process.text()).toContain("过程活动");
    expect(process.text()).toContain("查看 1");
    expect(process.text()).toContain("执行 1");
    expect(w.find('[data-testid="tool-card"]').exists()).toBe(false);
    expect(w.find('[data-testid="thinking-card"]').exists()).toBe(false);

    await process.get('[data-testid="process-activity-toggle"]').trigger("click");
    await flushPromises();
    expect(w.findAll('[data-testid="tool-card"]')).toHaveLength(2);
    expect(w.findAll('[data-testid="thinking-card"]')).toHaveLength(3);
    expect(w.get('[data-testid="assistant-message"]').text()).toContain("检查完成");
    w.unmount();
  });

  it("titles a folded explore batch in Chinese", async () => {
    const w = await mountConversation({
      cursor: 2,
      events: [
        fixtureToolDelta(1, {
          toolCallId: "a",
          title: "A",
          kind: "read",
          status: "completed",
        }),
        fixtureToolDelta(2, {
          toolCallId: "b",
          title: "B",
          kind: "read",
          status: "completed",
        }),
      ],
    });
    expect(w.get('[data-testid="tool-card"]').text()).toContain("已查看 2 项");
    expect(w.get('[data-testid="tool-card"]').text()).not.toContain("Explored");
    w.unmount();
  });

  it("renders Ask as a user decision card and submits the selected answer", async () => {
    const commands: DesktopCommand[] = [];
    const w = await mountConversation(
      {
        cursor: 1,
        status: "idle",
        events: [
          fixtureToolDelta(1, {
            toolCallId: "ask-1",
            title: "Ask: 是否包含模拟器 E2E？",
            kind: "ask",
            status: "running",
            inputSummary: JSON.stringify({
              question: "是否包含模拟器 E2E？",
              choices: ["包含", "暂不包含"],
            }),
            inputRedacted: false,
          }),
        ],
      },
      (command) => {
        commands.push(command);
        return { success: "true", data: { acknowledged: true } };
      },
    );

    const card = w.get('[data-testid="ask-card"]');
    expect(card.text()).toContain("需要你的确认");
    expect(card.text()).toContain("是否包含模拟器 E2E？");
    expect(card.findAll('[data-testid="ask-choice"]').map((item) => item.text())).toEqual([
      "包含",
      "暂不包含",
    ]);

    await card.findAll('[data-testid="ask-choice"]')[0]!.trigger("click");
    await flushPromises();
    expect(card.text()).toContain("已提交：包含");
    expect(commands).toContainEqual(
      expect.objectContaining({
        type: "turn.send",
        payload: expect.objectContaining({ message: "包含" }),
      }),
    );
    w.unmount();
  });

  it("keeps Ask choices available when the answer fails to send", async () => {
    const w = await mountConversation(
      {
        cursor: 1,
        status: "idle",
        events: [
          fixtureToolDelta(1, {
            toolCallId: "ask-failed",
            title: "Ask: 是否继续？",
            kind: "ask",
            status: "running",
            inputSummary: JSON.stringify({ question: "是否继续？", choices: ["继续"] }),
            inputRedacted: false,
          }),
        ],
      },
      (command) =>
        command.type === "turn.send"
          ? {
              success: "false",
              error: fakeError({ message: "Bridge 暂时不可用", retryable: true }),
            }
          : { success: "true", data: { acknowledged: true } },
    );

    const choice = w.get('[data-testid="ask-choice"]');
    await choice.trigger("click");
    await flushPromises();

    expect(w.get('[data-testid="ask-card"]').text()).not.toContain("已提交：");
    expect(choice.attributes("disabled")).toBeUndefined();
    expect(w.text()).toContain("Bridge 暂时不可用");
    w.unmount();
  });

  it("keeps heartbeats, snapshots, and stopped rows off the timeline", async () => {
    const w = await mountConversation({
      cursor: 4,
      events: [
        fixtureActivity(1, "heartbeat", "心跳 1"),
        fixtureActivity(2, "status", "快照完成"),
        fixtureTaskState(3, "idle", { reason: "cancelled" }),
        fixtureChanges(4),
      ],
      status: "idle",
    });
    expect(w.text()).not.toContain("心跳");
    expect(w.text()).not.toContain("快照完成");
    expect(w.text()).not.toContain("已停止");
    expect(w.get('[data-testid="change-whisper"]').text()).toMatch(/文件变更|工作区变更/);
    w.unmount();
  });

  it("clusters work-card duration with icon copy and collapse", async () => {
    const w = await mountConversation({
      cursor: 1,
      events: [
        fixtureToolDelta(1, {
          toolCallId: "exec-1",
          title: "npm test",
          kind: "execute",
          status: "completed",
          durationMs: 1500,
          resultSummary: "exit 0",
        }),
      ],
    });
    const card = w.get('[data-testid="tool-card"]');
    const cluster = w.get('[data-testid="tool-actions"]');
    expect(cluster.get('[data-testid="tool-duration"]').text()).toContain("1.5s");
    expect(cluster.get('[data-testid="tool-copy"]').attributes("aria-label")).toBe("复制摘要");
    expect(cluster.get('[data-testid="tool-toggle"]').attributes("aria-label")).toMatch(/展开|收起/);
    expect(card.text()).not.toContain("复制摘要");
    expect(card.text()).not.toContain("展开");
    w.unmount();
  });

  it("keeps a collapsed process row to a short path instead of the raw input dump", async () => {
    const w = await mountConversation({
      cursor: 1,
      events: [
        fixtureToolDelta(1, {
          toolCallId: "read-1",
          title: "read_file",
          kind: "read",
          status: "completed",
          locations: ["D:\\codex\\grok acp gui\\docs\\02-UI-UX-DESIGN.md"],
          inputSummary:
            '{"path":"D:\\\\codex\\\\grok acp gui\\\\docs\\\\02-UI-UX-DESIGN.md","limit":100}',
          resultSummary: "ok",
        }),
      ],
    });
    const card = w.get('[data-testid="tool-card"]');
    expect(card.classes()).not.toContain("surface-card");
    expect(card.text()).toContain("read_file");
    expect(card.text()).toContain("docs/02-UI-UX-DESIGN.md");
    expect(card.text()).not.toContain("limit");
    expect(card.text()).not.toContain('{"path"');
    await card.get('[data-testid="tool-toggle"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="tool-input-summary"]').text()).toContain("limit");
    w.unmount();
  });

  it("packs process rows tighter than the old 128px work-card slot", async () => {
    const w = await mountConversation({
      cursor: 3,
      events: [
        fixtureToolDelta(1, {
          toolCallId: "a",
          title: "list_dir",
          kind: "other",
          status: "completed",
        }),
        fixtureActivity(2, "thinking", "下一步"),
        fixtureToolDelta(3, {
          toolCallId: "b",
          title: "npm test",
          kind: "execute",
          status: "completed",
        }),
      ],
    });
    const rows = w.findAll(".virtual-row");
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      const style = row.attributes("style") ?? "";
      const minHeight = Number(/min-height:\s*(\d+)px/.exec(style)?.[1] ?? 999);
      expect(minHeight).toBeLessThanOrEqual(48);
    }
    w.unmount();
  });

  it("folds consecutive Grok Build read-only tool titles as one explore batch", async () => {
    const w = await mountConversation({
      cursor: 3,
      events: [
        fixtureToolDelta(1, {
          toolCallId: "a",
          title: "list_dir",
          kind: "other",
          status: "completed",
        }),
        fixtureToolDelta(2, {
          toolCallId: "b",
          title: "grep",
          kind: "other",
          status: "completed",
        }),
        fixtureToolDelta(3, {
          toolCallId: "c",
          title: "read_file",
          kind: "other",
          status: "completed",
        }),
      ],
    });
    expect(w.findAll('[data-testid="tool-card"]')).toHaveLength(1);
    expect(w.get('[data-testid="tool-card"]').text()).toContain("已查看 3 项");
    w.unmount();
  });

  it("shows relative time only after a gap and keeps exact time on hover", async () => {
    const early = "2026-08-05T12:00:00.000Z";
    const later = "2026-08-05T12:10:00.000Z";
    const w = await mountConversation({
      cursor: 3,
      events: [],
      items: [
        itemBase("user", 1, { text: "第一句", timestamp: early }),
        itemBase("assistant", 2, {
          text: "紧接着",
          streaming: false,
          frozen: true,
          timestamp: "2026-08-05T12:00:20.000Z",
        }),
        itemBase("assistant", 3, {
          text: "十分钟后",
          streaming: false,
          frozen: true,
          timestamp: later,
        }),
      ],
    });
    const first = w.get('[data-testid="timeline-item-user"]');
    const close = w.get('[data-seq="2"]');
    const gapped = w.get('[data-seq="3"]');
    expect(first.find('[data-testid="relative-time"]').exists()).toBe(true);
    expect(close.find('[data-testid="relative-time"]').exists()).toBe(false);
    expect(gapped.find('[data-testid="relative-time"]').exists()).toBe(true);
    expect(gapped.get('[data-testid="relative-time"]').attributes("title")).toBeTruthy();
    w.unmount();
  });
});

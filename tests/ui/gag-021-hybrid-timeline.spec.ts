import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import {
  FIX_SESSION,
  FIX_TASK,
  fixtureActivity,
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
) {
  const w = mount(ConversationView, {
    props: {
      bridge: createFakeDesktopBridge(),
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
    const w = await mountConversation({
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
    });
    const thumb = w.get('[data-testid="user-thumb"]');
    expect(thumb.attributes("width")).toBe("72");
    expect(thumb.attributes("height")).toBe("72");
    expect(w.get('[data-testid="user-message"]').text()).toContain("看这张图");
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
    expect(toggle.text()).toContain("思考中");
    expect(toggle.text()).not.toContain("Thinking");
    expect(w.find('[data-testid="thinking-body"]').exists()).toBe(false);
    await toggle.trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="thinking-body"]').text()).toBe("正在规划方案");
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

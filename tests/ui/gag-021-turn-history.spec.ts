import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import {
  FIX_SESSION,
  FIX_TASK,
  fixtureSessionSnapshot,
} from "../../src/features/conversation/fixtures";
import { useConversationStore } from "../../src/features/conversation/conversation-store";
import type { SessionTimelineSnapshot, TimelineItem } from "../../src/features/conversation/types";

function itemBase(kind: TimelineItem["kind"], seq: number, extra: object): TimelineItem {
  return {
    id: `${kind}-${seq}`,
    kind,
    seq,
    sessionId: FIX_SESSION,
    timestamp: "2026-08-14T11:00:00.000Z",
    eventKey: `${FIX_SESSION}:${seq}`,
    ...extra,
  } as TimelineItem;
}

async function mountConversation(snapshot: Partial<SessionTimelineSnapshot> = {}) {
  const wrapper = mount(ConversationView, {
    props: {
      bridge: createFakeDesktopBridge(),
      taskId: FIX_TASK,
      snapshot: fixtureSessionSnapshot(snapshot),
    },
    attachTo: document.body,
  });
  await flushPromises();
  return wrapper;
}

describe("GAG-021 turn history", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("opens the turn list from the task-bar clock", async () => {
    const wrapper = await mountConversation({
      cursor: 2,
      events: [],
      items: [
        itemBase("user", 1, { text: "请检查对比度" }),
        itemBase("assistant", 2, { text: "好的。", streaming: false, frozen: true }),
      ],
    });
    expect(wrapper.find('[data-testid="turn-list"]').exists()).toBe(false);
    await wrapper.get('[data-testid="turn-history"]').trigger("click");
    expect(wrapper.get('[data-testid="turn-list"]').isVisible()).toBe(true);
    wrapper.unmount();
  });

  it("lists sent user first lines only, without #seq or queued follow-ups", async () => {
    const wrapper = await mountConversation({
      status: "running",
      cursor: 4,
      events: [],
      items: [
        itemBase("user", 1, { text: "第一行目标\n后面还有细节" }),
        itemBase("assistant", 2, { text: "收到。", streaming: false, frozen: true }),
        itemBase("user", 3, { text: "第二轮补充" }),
        itemBase("assistant", 4, { text: "继续。", streaming: false, frozen: true }),
      ],
    });
    const store = useConversationStore();
    store.setDraft("queued follow-up that is not a turn");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="queue-bar"]').text()).toContain(
      "queued follow-up that is not a turn",
    );

    await wrapper.get('[data-testid="turn-history"]').trigger("click");
    const list = wrapper.get('[data-testid="turn-list"]');
    const rows = list.findAll('[data-testid="turn-row"]');
    expect(rows).toHaveLength(2);
    expect(rows[0]?.text()).toContain("第一行目标");
    expect(rows[0]?.text()).not.toContain("后面还有细节");
    expect(rows[1]?.text()).toContain("第二轮补充");
    expect(rows.every((row) => /刚刚|\d+ 分钟前|\d+ 小时前|\d+ 天前/.test(row.text()))).toBe(true);
    expect(list.text()).not.toMatch(/#\d+/);
    expect(list.text()).not.toContain("queued follow-up that is not a turn");
    expect(list.text()).not.toContain("收到。");
    wrapper.unmount();
  });

  it("scrolls to the user bubble when a turn row is clicked", async () => {
    const items: TimelineItem[] = [];
    for (let seq = 1; seq <= 16; seq += 1) {
      if (seq % 2 === 1) {
        items.push(itemBase("user", seq, { text: `轮次 ${seq}` }));
      } else {
        items.push(
          itemBase("assistant", seq, {
            text: `回复 ${seq}`,
            streaming: false,
            frozen: true,
          }),
        );
      }
    }
    const wrapper = await mountConversation({
      cursor: 16,
      events: [],
      items,
    });
    const list = wrapper.get('[data-testid="conversation-virtual-list"]');
    const listEl = list.element as HTMLElement;
    Object.defineProperty(listEl, "clientHeight", { configurable: true, value: 160 });
    listEl.scrollTop = 0;
    listEl.dispatchEvent(new Event("scroll"));
    await flushPromises();

    await wrapper.get('[data-testid="turn-history"]').trigger("click");
    const rows = wrapper.findAll('[data-testid="turn-row"]');
    await rows[rows.length - 1]!.trigger("click");
    await flushPromises();

    expect(listEl.scrollTop).toBeGreaterThan(0);
    wrapper.unmount();
  });

  it("keeps an empty conversation's turn list empty or disabled, without fixture copy", async () => {
    const wrapper = await mountConversation({
      title: "对话演示",
      status: "idle",
      cursor: 0,
      events: [],
      items: [],
    });
    const clock = wrapper.get('[data-testid="turn-history"]');
    expect((clock.element as HTMLButtonElement).disabled).toBe(true);
    await clock.trigger("click");
    expect(wrapper.find('[data-testid="turn-list"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("快照消息");
    expect(wrapper.findAll('[data-testid="turn-row"]')).toHaveLength(0);
    wrapper.unmount();
  });
});

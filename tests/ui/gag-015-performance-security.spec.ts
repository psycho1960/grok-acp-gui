import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import type { ProjectId, TaskId } from "../../src/bridge/types";
import {
  applyEvents,
  createEmptyConversationState,
} from "../../src/features/conversation/reducer";
import {
  FIX_TASK,
  fixtureAssistantDelta,
  generateManyEvents,
} from "../../src/features/conversation/fixtures";
import { renderSafeMarkdown } from "../../src/features/conversation/markdown";
import { groupTasks } from "../../src/features/task-center/grouping";
import { buildGroupedListRows } from "../../src/features/task-center/list-rows";
import type { TaskViewModel } from "../../src/features/task-center/types";
import VirtualList from "../../src/features/task-center/VirtualList.vue";
import {
  EvidenceRecorder,
  FakeDesktopBridgeScenario,
} from "../harness/gag-015-harness";

function tasks(count: number): TaskViewModel[] {
  const statuses = ["running", "waiting_permission", "merged", "interrupted"] as const;
  return Array.from({ length: count }, (_, index) => ({
    id: `gag-015-task-${index}` as TaskId,
    projectId: "gag-015-project" as ProjectId,
    projectLabel: "Isolated performance fixture",
    title: `Task ${index}`,
    status: statuses[index % statuses.length],
    workspaceKind: "worktree",
    createdAt: "2026-08-10T00:00:00.000Z",
    updatedAt: new Date(Date.UTC(2026, 7, 10, 0, 0, index % 60)).toISOString(),
    lastSeq: index,
  }));
}

describe("GAG-015 performance and active-content gates", () => {
  it("keeps 500 tasks bounded by the virtualized DOM", async () => {
    const evidence = new EvidenceRecorder();
    const started = performance.now();
    const rows = buildGroupedListRows(groupTasks(tasks(500)));
    const groupedMs = performance.now() - started;
    evidence.record({ name: "group-500-tasks", value: groupedMs, unit: "ms" });

    const wrapper = mount(VirtualList, {
      props: {
        items: rows,
        itemHeight: 72,
        getKey: (row: (typeof rows)[number]) => row.key,
      },
      slots: { default: "<div class='task-row'>task</div>" },
      attachTo: document.body,
    });
    await wrapper.vm.$nextTick();

    expect(rows.length).toBe(504);
    expect(groupedMs).toBeLessThan(500);
    expect(wrapper.findAll(".virtual-list-row").length).toBeGreaterThan(0);
    expect(wrapper.findAll(".virtual-list-row").length).toBeLessThan(50);
    expect(wrapper.get('[role="list"]').attributes("aria-label")).toBe("任务列表");
    console.info(JSON.stringify({
      metric: "gag015.task-list-500",
      sampleCount: 1,
      durationMs: groupedMs,
      thresholdMs: 500,
      rowCount: rows.length,
      mountedDomRows: wrapper.findAll(".virtual-list-row").length,
      domThreshold: 50,
    }));
    wrapper.unmount();
  });

  it("processes 10k timeline events within the documented budget", () => {
    const started = performance.now();
    const state = applyEvents(
      createEmptyConversationState(FIX_TASK),
      generateManyEvents(10_000),
    );
    const durationMs = performance.now() - started;
    console.info(JSON.stringify({
      metric: "gag015.timeline-10000",
      sampleCount: 1,
      durationMs,
      thresholdMs: 5_000,
      finalSequence: state.cursor.lastSeq,
    }));
    expect(durationMs).toBeLessThan(5_000);
    expect(state.cursor.lastSeq).toBe(10_000);
  });

  it("orders a reversed 100-delta burst without losing or crossing sequence", () => {
    const burst = Array.from({ length: 99 }, (_, index) => 100 - index)
      .map((seq) => fixtureAssistantDelta(seq, `[${seq}]`));
    burst.push(fixtureAssistantDelta(1, "[1]"));
    const state = applyEvents(createEmptyConversationState(FIX_TASK), burst);

    expect(state.cursor.lastSeq).toBe(100);
    expect(state.pendingEvents.size).toBe(0);
    expect(state.needsSnapshotRefresh).toBe(false);
    console.info(JSON.stringify({
      metric: "gag015.reversed-delta-burst",
      sampleCount: 1,
      eventCount: 100,
      finalSequence: state.cursor.lastSeq,
      pendingEvents: state.pendingEvents.size,
    }));
    const assistant = state.items.find((item) => item.kind === "assistant");
    expect(assistant?.kind).toBe("assistant");
    if (assistant?.kind === "assistant") {
      expect(assistant.text).toBe(
        Array.from({ length: 100 }, (_, index) => `[${index + 1}]`).join(""),
      );
    }
  });

  it("isolates DesktopBridge scenarios and strips hostile active content", async () => {
    const first = new FakeDesktopBridgeScenario("first");
    const second = new FakeDesktopBridgeScenario("second");
    const firstEvents: string[] = [];
    const secondEvents: string[] = [];
    await first.bridge.subscribe((event) => firstEvents.push(event.type));
    await second.bridge.subscribe((event) => secondEvents.push(event.type));
    first.emit(fixtureAssistantDelta(1, "safe"));
    expect(firstEvents).toEqual(["message.delta"]);
    expect(secondEvents).toEqual([]);

    const html = renderSafeMarkdown(
      '<svg onload="alert(1)"></svg> [bad](javascript:alert(1)) [data](data:text/html,x)',
    );
    expect(html).not.toMatch(/<svg|href="(?:javascript:|data:text\/html)/i);
    expect(html).toContain("&lt;svg");
  });
});

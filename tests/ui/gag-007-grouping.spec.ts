import { describe, expect, it } from "vitest";
import type { ProjectId, TaskId } from "../../src/bridge/types";
import {
  compareTasks,
  countByGroup,
  filterAndSortTasks,
  groupTasks,
  matchesFilters,
} from "../../src/features/task-center/grouping";
import {
  buildTaskCenterHash,
  parseTaskCenterHash,
} from "../../src/features/task-center/hash-route";
import type { TaskViewModel } from "../../src/features/task-center/types";
import { DEFAULT_FILTERS } from "../../src/features/task-center/types";

function task(
  partial: Partial<TaskViewModel> & Pick<TaskViewModel, "id" | "status" | "title" | "updatedAt">,
): TaskViewModel {
  return {
    projectId: "p1" as ProjectId,
    projectLabel: "Alpha",
    workspaceKind: "worktree",
    createdAt: "2026-04-01T08:00:00.000Z",
    lastSeq: 0,
    ...partial,
  };
}

const sample: TaskViewModel[] = [
  task({
    id: "c" as TaskId,
    title: "Completed doc",
    status: "merged",
    updatedAt: "2026-04-01T12:00:00.000Z",
  }),
  task({
    id: "r2" as TaskId,
    title: "Running late",
    status: "running",
    updatedAt: "2026-04-01T11:00:00.000Z",
  }),
  task({
    id: "r1" as TaskId,
    title: "Running early",
    status: "running",
    updatedAt: "2026-04-01T13:00:00.000Z",
  }),
  task({
    id: "w" as TaskId,
    title: "Needs permission",
    status: "waiting_permission",
    updatedAt: "2026-04-01T10:00:00.000Z",
  }),
  task({
    id: "i" as TaskId,
    title: "Interrupted",
    status: "interrupted",
    updatedAt: "2026-04-01T09:00:00.000Z",
    projectId: "p2" as ProjectId,
    projectLabel: "Beta",
  }),
];

describe("GAG-007 grouping / sort / filter", () => {
  it("sorts needs-attention first, then running by updatedAt desc, stable by id", () => {
    const sorted = [...sample].sort(compareTasks);
    expect(sorted.map((t) => t.id)).toEqual(["w", "r1", "r2", "c", "i"]);
  });

  it("groups into the four Task Center buckets", () => {
    const groups = groupTasks(sample);
    expect(groups.map((g) => g.id)).toEqual([
      "needs_attention",
      "running",
      "completed",
      "failed_interrupted",
    ]);
    expect(groups[0].tasks.map((t) => t.id)).toEqual(["w"]);
    expect(groups[1].tasks.map((t) => t.id)).toEqual(["r1", "r2"]);
    expect(countByGroup(sample)).toEqual({
      needs_attention: 1,
      running: 2,
      completed: 1,
      failed_interrupted: 1,
    });
  });

  it("filters by query, status, project, and group", () => {
    const byQuery = filterAndSortTasks(sample, {
      ...DEFAULT_FILTERS,
      query: "permission",
    });
    expect(byQuery.map((t) => t.id)).toEqual(["w"]);

    const byStatus = filterAndSortTasks(sample, {
      ...DEFAULT_FILTERS,
      status: "running",
    });
    expect(byStatus.map((t) => t.id)).toEqual(["r1", "r2"]);

    const byProject = filterAndSortTasks(sample, {
      ...DEFAULT_FILTERS,
      projectId: "p2" as ProjectId,
    });
    expect(byProject.map((t) => t.id)).toEqual(["i"]);

    const byGroup = filterAndSortTasks(sample, {
      ...DEFAULT_FILTERS,
      group: "failed_interrupted",
    });
    expect(byGroup.map((t) => t.id)).toEqual(["i"]);
  });

  it("matches updatedWithin windows", () => {
    const now = Date.parse("2026-04-01T12:30:00.000Z");
    expect(
      matchesFilters(
        sample[0],
        { ...DEFAULT_FILTERS, updatedWithin: "1h" },
        now,
      ),
    ).toBe(true);
    expect(
      matchesFilters(
        sample[4],
        { ...DEFAULT_FILTERS, updatedWithin: "1h" },
        now,
      ),
    ).toBe(false);
  });
});

describe("GAG-007 hash deep links", () => {
  it("parses and builds task-center routes", () => {
    expect(parseTaskCenterHash("#task-center")).toEqual({
      active: true,
      taskId: null,
    });
    expect(parseTaskCenterHash("#task-center/task-run-1")).toEqual({
      active: true,
      taskId: "task-run-1",
    });
    expect(parseTaskCenterHash("#shell")).toEqual({
      active: false,
      taskId: null,
    });
    expect(buildTaskCenterHash()).toBe("#task-center");
    expect(buildTaskCenterHash("task-1")).toBe("#task-center/task-1");
  });
});

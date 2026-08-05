import { describe, expect, it } from "vitest";
import {
  buildConversationHash,
  parseConversationHash,
} from "../../src/features/conversation/hash-route";

describe("GAG-008 conversation hash routes", () => {
  it("parses bare, task, and event deep links", () => {
    expect(parseConversationHash("#conversation")).toEqual({
      active: true,
      taskId: null,
      eventSeq: null,
    });
    expect(parseConversationHash("#conversation/task-1")).toEqual({
      active: true,
      taskId: "task-1",
      eventSeq: null,
    });
    expect(parseConversationHash("#conversation/task-1/e/42")).toEqual({
      active: true,
      taskId: "task-1",
      eventSeq: 42,
    });
    expect(parseConversationHash("#conversation/task-1?event=7")).toEqual({
      active: true,
      taskId: "task-1",
      eventSeq: 7,
    });
  });

  it("builds hashes", () => {
    expect(buildConversationHash()).toBe("#conversation");
    expect(buildConversationHash("t1")).toBe("#conversation/t1");
    expect(buildConversationHash("t1", 9)).toBe("#conversation/t1/e/9");
  });

  it("ignores non-conversation hashes", () => {
    expect(parseConversationHash("#task-center/x").active).toBe(false);
  });
});

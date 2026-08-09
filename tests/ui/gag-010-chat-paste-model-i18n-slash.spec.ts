// GAG-010 / goal: clipboard screenshot paste, in-conversation model &
// reasoning switching, optional task title with auto-derivation, slash
// command menu, and a full-Chinese UI audit.
//
// These tests drive the real shipped components and pure logic modules.

import { createPinia, setActivePinia } from "pinia";
import { mount, flushPromises } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type {
  ArtifactBlobInput,
  DesktopCommand,
  ReasoningEffort,
  SlashCommandInfo,
} from "../../src/bridge/types";
import Composer from "../../src/features/conversation/Composer.vue";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import { useConversationStore } from "../../src/features/conversation/conversation-store";
import { fixtureSessionSnapshot, FIX_TASK } from "../../src/features/conversation/fixtures";
import {
  extractImageFiles,
  imageFileToBlobInput,
  sanitizeDisplayName,
} from "../../src/features/conversation/clipboard-images";
import {
  filterSlashCommands,
  insertSlashCommand,
  slashMenuState,
} from "../../src/features/conversation/slash-commands";
import CreateTaskDialog from "../../src/features/task-center/CreateTaskDialog.vue";
import { deriveTaskTitle } from "../../src/features/task-center/title";

// ---------------------------------------------------------------------------
// 1. Clipboard image extraction (pure logic)
// ---------------------------------------------------------------------------

/**
 * Type into the textarea keeping the caret at the end. The wrapper's
 * `modelValue` prop is updated alongside so Vue does not patch the DOM value
 * back to the stale prop (the real parent updates the prop via the emit).
 */
async function typeIn(
  input: { element: HTMLTextAreaElement; trigger: (event: string, options?: unknown) => Promise<unknown> },
  wrapper: ReturnType<typeof mount> & { setProps: (props: Record<string, unknown>) => Promise<void> },
  value: string,
): Promise<void> {
  const el = input.element as HTMLTextAreaElement;
  el.value = value;
  el.setSelectionRange(value.length, value.length);
  await input.trigger("input");
  await wrapper.setProps({ modelValue: value });
  await input.trigger("keyup");
  await wrapper.vm.$nextTick();
}

/** Dispatch a real paste event with a synthetic DataTransfer shim. */
function dispatchPaste(target: Element, dataTransfer: DataTransfer): void {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: dataTransfer });
  target.dispatchEvent(event);
}

/** Let happy-dom FileReader macrotasks settle. */
async function settlePaste(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 20));
  await flushPromises();
}

function makeDataTransfer(files: Array<{ file: File; kind?: string }>): DataTransfer {
  const items = files.map(({ file, kind = "file" }) => ({
    kind,
    getAsFile: () => file,
    type: file.type,
  }));
  return {
    files: files.map((entry) => entry.file) as unknown as FileList,
    items: items as unknown as DataTransferItemList,
  } as DataTransfer;
}

function makePngFile(name = "image.png", type = "image/png"): File {
  return new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], name, {
    type,
  });
}

describe("clipboard-images", () => {
  it("extracts image blob items without a filesystem path", () => {
    const png = makePngFile();
    const dt = makeDataTransfer([{ file: png }]);
    const images = extractImageFiles(dt);
    expect(images).toHaveLength(1);
    expect(images[0].displayName).toBe("image.png");
    expect("path" in images[0].file).toBe(false);
  });

  it("skips non-image files and dedupes identical entries", () => {
    const png = makePngFile();
    const txt = new File(["hello"], "note.txt", { type: "text/plain" });
    const dt = makeDataTransfer([
      { file: png },
      { file: png }, // duplicate
      { file: txt }, // not an image
    ]);
    expect(extractImageFiles(dt)).toHaveLength(1);
  });

  it("names unnamed clipboard blobs 剪贴板图片.<ext>", () => {
    const dt = makeDataTransfer([{ file: new File([new Uint8Array([1])], "", { type: "image/png" }) }]);
    const images = extractImageFiles(dt);
    expect(images[0].displayName).toBe("剪贴板图片.png");
  });

  it("sanitizes display names and converts files to bridge blob input", async () => {
    expect(sanitizeDisplayName("a/b\\c:d.png")).toBe("abcd.png");
    const input: ArtifactBlobInput = await imageFileToBlobInput({
      file: makePngFile(),
      displayName: "截图.png",
    });
    expect(input.displayName).toBe("截图.png");
    expect(input.base64Data).toMatch(/^iVBORw0KG/);
  });

  it("rejects oversized clipboard images", async () => {
    const big = new File([new Uint8Array(7 * 1024 * 1024)], "big.png", {
      type: "image/png",
    });
    await expect(
      imageFileToBlobInput({ file: big, displayName: "big.png" }),
    ).rejects.toThrow(/6 MiB/);
  });
});

// ---------------------------------------------------------------------------
// 2. Slash command matching / filtering / insertion (pure logic)
// ---------------------------------------------------------------------------

const SEED_COMMANDS: SlashCommandInfo[] = [
  { name: "init", description: "初始化一个新项目", acceptsInput: false },
  { name: "plan", description: "为变更制定计划", acceptsInput: true },
  { name: "share", description: "分享当前会话", acceptsInput: false },
];

describe("slash-commands", () => {
  it("opens only when the current line starts with /", () => {
    expect(slashMenuState("", 0).open).toBe(false);
    // A bare "/" opens the menu with an empty query (full command list).
    const bare = slashMenuState("/", 1);
    expect(bare.open).toBe(true);
    expect(bare.query).toBe("");
    const state = slashMenuState("/pl", 3);
    expect(state.open).toBe(true);
    expect(state.query).toBe("pl");
    expect(state.lineStart).toBe(0);
    // Multi-line: the / line is the second line.
    const second = slashMenuState("first line\n/ini", "first line\n/ini".length);
    expect(second.open).toBe(true);
    expect(second.query).toBe("ini");
    expect(second.lineStart).toBe(11);
    // Mid-line / does not open the menu.
    expect(slashMenuState("abc/def", 4).open).toBe(false);
    // Whitespace after the command name closes the menu.
    expect(slashMenuState("/plan ", 6).open).toBe(false);
  });

  it("filters by prefix case-insensitively", () => {
    expect(filterSlashCommands(SEED_COMMANDS, "")).toHaveLength(3);
    expect(filterSlashCommands(SEED_COMMANDS, "p").map((c) => c.name)).toEqual(["plan"]);
    expect(filterSlashCommands(SEED_COMMANDS, "IN").map((c) => c.name)).toEqual(["init"]);
    expect(filterSlashCommands(SEED_COMMANDS, "zzz")).toHaveLength(0);
  });

  it("inserts the command text and returns the caret position", () => {
    const inserted = insertSlashCommand("/pl", 3, "plan");
    expect(inserted.text).toBe("/plan");
    expect(inserted.cursor).toBe(5);
    // Replaces the whole line, keeping other lines intact.
    const multi = insertSlashCommand("first\n/sha", 9, "share");
    expect(multi.text).toBe("first\n/share");
    expect(multi.cursor).toBe(12);
  });
});

// ---------------------------------------------------------------------------
// 3. Task title derivation (pure logic)
// ---------------------------------------------------------------------------

describe("deriveTaskTitle", () => {
  it("uses the first sentence of the first line", () => {
    expect(deriveTaskTitle("实现登录页面。包含邮箱和密码校验。")).toBe("实现登录页面");
  });

  it("falls back to the first non-empty line", () => {
    expect(deriveTaskTitle("\n第二行开始\n还是第二行")).toBe("第二行开始");
  });

  it("truncates long sentences with an ellipsis", () => {
    const long = `优化${"长".repeat(80)}`;
    const title = deriveTaskTitle(long);
    expect(title.endsWith("…")).toBe(true);
    expect(title.length).toBeLessThanOrEqual(31);
  });

  it("never returns the placeholder 未命名任务", () => {
    expect(deriveTaskTitle("写一个脚本")).not.toContain("未命名任务");
    expect(deriveTaskTitle("   ")).toBe("新任务");
  });
});

// ---------------------------------------------------------------------------
// 4. Real Composer: paste → attachment chip through the store import path
// ---------------------------------------------------------------------------

describe("composer clipboard paste", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
    sessionStorage.clear();
  });

  it("pastes a clipboard image and the chip reaches the pending list", async () => {
    const commands: DesktopCommand[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        commands.push(command);
        if (command.type === "artifact.import.blob") {
          const blobs = command.payload.blobs as ArtifactBlobInput[];
          return {
            success: "true",
            data: {
              artifacts: blobs.map((blob, index) => ({
                artifactId: `artifact-clip-${index}`,
                displayName: blob.displayName,
                mimeType: "image/png",
                bytes: 1024,
                state: "ready",
                previewCapability: "inline",
              })),
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });

    const wrapper = mount(ConversationView, {
      props: {
        bridge,
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({ status: "idle", cursor: 0, events: [] }),
      },
      attachTo: document.body,
    });
    await flushPromises();

    const textarea = wrapper.get('[data-testid="composer-input"]');
    const dt = makeDataTransfer([{ file: makePngFile() }]);
    dispatchPaste(textarea.element, dt);
    await settlePaste();

    const store = useConversationStore();
    expect(commands.some((c) => c.type === "artifact.import.blob")).toBe(true);
    expect(store.attachments).toHaveLength(1);
    expect(store.attachments[0].displayName).toBe("image.png");
    // The chip is visible in the composer.
    const chips = wrapper.findAll(".attachment-list li");
    expect(chips.length).toBe(1);
    expect(chips[0].text()).toContain("image.png");
    wrapper.unmount();
  });

  it("pasting a non-image clipboard item does nothing", async () => {
    const commands: DesktopCommand[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        commands.push(command);
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const wrapper = mount(ConversationView, {
      props: {
        bridge,
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({ status: "idle", cursor: 0, events: [] }),
      },
      attachTo: document.body,
    });
    await flushPromises();
    const textarea = wrapper.get('[data-testid="composer-input"]');
    const dt = makeDataTransfer([{ file: new File(["hi"], "note.txt", { type: "text/plain" }) }]);
    dispatchPaste(textarea.element, dt);
    await settlePaste();
    expect(commands.some((c) => c.type === "artifact.import.blob")).toBe(false);
    wrapper.unmount();
  });
});

// ---------------------------------------------------------------------------
// 5. Real Composer: "/" quick-command menu
// ---------------------------------------------------------------------------

describe("composer slash command menu", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  function mountComposer(commands: SlashCommandInfo[]) {
    return mount(Composer, {
      props: {
        modelValue: "",
        capabilities: { canSend: true, canCancel: true, bridgeOnline: true },
        slashCommands: commands,
      },
      attachTo: document.body,
    });
  }

  it("opens on /, filters by prefix, selects on Enter, closes on Esc", async () => {
    const wrapper = mountComposer(SEED_COMMANDS);
    const input = wrapper.get('[data-testid="composer-input"]');

    await typeIn(input, wrapper, "/");
    // A bare "/" opens the menu with the full command list.
    expect(wrapper.find('[data-testid="slash-menu"]').exists()).toBe(true);
    expect(wrapper.findAll('[data-testid="slash-menu-item"]')).toHaveLength(3);

    await typeIn(input, wrapper, "/p");
    expect(wrapper.find('[data-testid="slash-menu"]').exists()).toBe(true);
    const items = wrapper.findAll('[data-testid="slash-menu-item"]');
    expect(items.map((item) => item.text())).toEqual([
      expect.stringContaining("/plan"),
    ]);

    await input.trigger("keydown", { key: "Enter" });
    // The parent receives update:modelValue and patches the prop back.
    await wrapper.setProps({ modelValue: "/plan" });
    await wrapper.vm.$nextTick();
    expect(wrapper.get('[data-testid="composer-input"]').element).toHaveProperty(
      "value",
      "/plan",
    );
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual(["/plan"]);
    expect(wrapper.find('[data-testid="slash-menu"]').exists()).toBe(false);

    // Esc closes the menu without cancel
    await typeIn(input, wrapper, "/s");
    expect(wrapper.find('[data-testid="slash-menu"]').exists()).toBe(true);
    await input.trigger("keydown", { key: "Escape" });
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="slash-menu"]').exists()).toBe(false);
    expect(wrapper.emitted("cancel")).toBeFalsy();
    wrapper.unmount();
  });

  it("navigates with arrow keys and keeps other lines intact", async () => {
    const wrapper = mountComposer(SEED_COMMANDS);
    const input = wrapper.get('[data-testid="composer-input"]');
    await typeIn(input, wrapper, "line one\n/i");
    expect(wrapper.findAll('[data-testid="slash-menu-item"]')).toHaveLength(1);

    await typeIn(input, wrapper, "line one\n/");
    expect(wrapper.findAll('[data-testid="slash-menu-item"]')).toHaveLength(3);

    await input.trigger("keydown", { key: "ArrowDown" });
    await input.trigger("keydown", { key: "Enter" });
    // Read what the component emitted (the parent would patch the prop).
    const inserted = wrapper.emitted("update:modelValue")?.at(-1)?.[0] as string;
    await wrapper.setProps({ modelValue: inserted });
    await wrapper.vm.$nextTick();
    expect((input.element as HTMLTextAreaElement).value).toBe(inserted);
    expect(inserted.startsWith("line one\n/")).toBe(true);
    expect(inserted.length).toBeGreaterThan("line one\n/".length);
    wrapper.unmount();
  });

  it("shows an empty state when no command matches", async () => {
    const wrapper = mountComposer(SEED_COMMANDS);
    const input = wrapper.get('[data-testid="composer-input"]');
    await typeIn(input, wrapper, "/zzz");
    expect(wrapper.find('[data-testid="slash-menu"]').exists()).toBe(true);
    expect(wrapper.text()).toContain("没有匹配的快捷指令");
    wrapper.unmount();
  });
});

// ---------------------------------------------------------------------------
// 6. Model & reasoning switching persists and drives the next turn
// ---------------------------------------------------------------------------

describe("conversation model & reasoning settings", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
    sessionStorage.clear();
  });

  it("persists selection via session.configure and restores on reopen", async () => {
    const configureCalls: Array<{ model?: string | null; reasoning?: string }> = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "session.configure") {
          const settings = command.payload.settings as Record<string, string>;
          configureCalls.push({
            model: settings.model ?? null,
            reasoning: settings.reasoning,
          });
          return { success: "true", data: { acknowledged: "session.configure" } };
        }
        if (command.type === "task.open") {
          return {
            success: "true",
            data: {
              taskId: command.payload.taskId,
              title: "Reopened",
              status: "idle",
              model: "deepseek",
              reasoning: "max",
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });

    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "idle" }));

    await store.configureModel("deepseek");
    await store.configureReasoning("max");
    expect(configureCalls).toEqual([
      { model: "deepseek", reasoning: undefined },
      { model: null, reasoning: "max" },
    ]);
    expect(store.selectedModel).toBe("deepseek");
    expect(store.selectedReasoning).toBe("max");

    // Reopening the task restores the persisted selection from task.open.
    await store.openTask(FIX_TASK);
    expect(store.selectedModel).toBe("deepseek");
    expect(store.selectedReasoning).toBe("max");

    // Failure reverts the local selection.
    let fail = false;
    const failingBridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "session.configure") {
          if (fail) {
            return {
              success: "false",
              error: { code: "DB_QUERY_FAILED", message: "保存失败", retryable: false, detailsRedacted: true, correlationId: "x" as never },
            };
          }
          return { success: "true", data: {} };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store2 = useConversationStore();
    await store2.attach(failingBridge);
    store2.openFromSnapshot(fixtureSessionSnapshot({ status: "idle" }));
    await store2.configureReasoning("high");
    expect(store2.selectedReasoning).toBe("high");
    fail = true;
    const ok = await store2.configureReasoning("low");
    expect(ok).toBe(false);
    expect(store2.selectedReasoning).toBe("high");
  });

  it("header selects drive the store and slash menu uses seeded commands", async () => {
    const configureCalls: string[] = [];
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: {
        capabilities: { models: [], modes: [], slashCommands: SEED_COMMANDS },
      },
      onExecute(command) {
        if (command.type === "session.configure") configureCalls.push(command.type);
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const wrapper = mount(ConversationView, {
      props: {
        bridge,
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({ status: "idle", cursor: 0, events: [] }),
      },
      attachTo: document.body,
    });
    await flushPromises();

    const modelSelect = wrapper.get('[data-testid="conversation-model-select"] select');
    await modelSelect.setValue("deepseek");
    await flushPromises();
    expect(configureCalls).toContain("session.configure");

    const reasoningSelect = wrapper.get('[data-testid="conversation-reasoning-select"] select');
    await reasoningSelect.setValue("max");
    await flushPromises();
    expect(wrapper.get('[data-testid="conversation-reasoning-select"]').text()).toContain("推理强度");

    // The slash menu receives the store's seeded commands through the view.
    const store = useConversationStore();
    store.setDraft("/ini");
    await wrapper.vm.$nextTick();
    const input = wrapper.get('[data-testid="composer-input"]');
    const el = input.element as HTMLTextAreaElement;
    el.setSelectionRange(el.value.length, el.value.length);
    await input.trigger("keyup");
    await wrapper.vm.$nextTick();
    expect(wrapper.findAll('[data-testid="slash-menu-item"]').length).toBe(1);
    wrapper.unmount();
  });
});

// ---------------------------------------------------------------------------
// 7. CreateTaskDialog: title optional + auto-generated from first sentence
// ---------------------------------------------------------------------------

describe("CreateTaskDialog optional title", () => {
  it("marks the title as optional and derives it from the first sentence", async () => {
    const wrapper = mount(CreateTaskDialog, {
      props: { open: true },
      attachTo: document.body,
    });
    await wrapper.vm.$nextTick();

    const label = wrapper.get('[data-testid="create-task-title"]');
    expect(label.text()).toContain("标题（可选）");

    await wrapper.get('[data-testid="create-task-prompt"] textarea').setValue(
      "实现登录页面。包含邮箱和密码校验。",
    );
    await wrapper.vm.$nextTick();
    // No required-field error is shown.
    expect(wrapper.find('[data-testid="create-task-error"]').exists()).toBe(false);

    await wrapper.get('[data-testid="create-task-submit"]').trigger("click");
    const emitted = wrapper.emitted("create")?.at(-1)?.[0] as {
      title: string;
      prompt: string;
      reasoning: ReasoningEffort;
    };
    expect(emitted.title).toBe("实现登录页面");
    expect(emitted.title).not.toBe("未命名任务");
    expect(emitted.prompt).toBe("实现登录页面。包含邮箱和密码校验。");
    expect(emitted.reasoning).toBe("medium");
    wrapper.unmount();
  });

  it("prefers an explicit title when provided", async () => {
    const wrapper = mount(CreateTaskDialog, {
      props: { open: true },
      attachTo: document.body,
    });
    await wrapper.vm.$nextTick();
    await wrapper.get('[data-testid="create-task-prompt"] textarea').setValue("优化启动性能");
    await wrapper.get('[data-testid="create-task-title"] input').setValue("性能优化");
    await wrapper.get('[data-testid="create-task-submit"]').trigger("click");
    const emitted = wrapper.emitted("create")?.at(-1)?.[0] as { title: string };
    expect(emitted.title).toBe("性能优化");
    wrapper.unmount();
  });
});

// ---------------------------------------------------------------------------
// 8. Full-Chinese UI audit: no English residues in shipped templates
// ---------------------------------------------------------------------------

describe("全中文界面静态审计", () => {
  const SRC_ROOT = resolve(__dirname, "../../src");
  const BANNED_WORDS = [
    /\bInspector\b/,
    /\bArtifacts?\b/,
    /\bPlan\b/,
    /\battempt\b/,
    /\bAgent\b/,
    /\bAsk\b/,
    /\bLow\b/,
    /\bMedium\b/,
    /\bHigh\b/,
    /\bMax\b/,
    /\bDialog\b/,
    /\bDrawer\b/,
    /\bCancel\b/,
    /\bConfirm\b/,
    /\bButtons?\b/,
    /\bFields?\b/,
    /\bStatus\b/,
    /\bNeutral\b/,
    /\bSuccess\b/,
    /\bWarning\b/,
    /\bError\b/,
  ];

  function collectTemplateFiles(dir: string, out: string[] = []): string[] {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === "node_modules") continue;
      const full = join(dir, entry.name);
      if (entry.isDirectory()) collectTemplateFiles(full, out);
      else if (entry.name.endsWith(".vue")) out.push(full);
    }
    return out;
  }

  it("no banned English words in visible template text or aria/placeholder/label values", () => {
    const files = collectTemplateFiles(SRC_ROOT);
    expect(files.length).toBeGreaterThan(10);
    const offenders: string[] = [];

    for (const file of files) {
      const source = readFileSync(file, "utf8");
      const template = source.match(/<template>([\s\S]*?)<\/template>/)?.[1] ?? "";
      if (!template) continue;

      // Attribute values that are user-visible: aria-label, placeholder, label,
      // title (on dialog), description (on dialog).
      const attrValues = [
        ...template.matchAll(/(?:aria-label|placeholder|label|description)="([^"]*)"/g),
      ].map((m) => m[1]);
      // Plain text nodes (between > and <).
      const textNodes = [
        ...template.matchAll(/>([^<>{}]*[A-Za-z][^<>{}]*)</g),
      ].map((m) => m[1].trim()).filter(Boolean);

      for (const value of [...attrValues, ...textNodes]) {
        for (const banned of BANNED_WORDS) {
          if (banned.test(value)) {
            offenders.push(`${file}: "${value}" matches ${banned}`);
          }
        }
      }
    }
    expect(offenders).toEqual([]);
  });
});

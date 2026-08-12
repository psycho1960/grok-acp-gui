import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import ToastHost from "../../src/shared/ui/ToastHost.vue";
import { toast } from "../../src/shared/ui/toast";

describe("GAG-017 toast system", () => {
  afterEach(() => {
    toast.clear();
    vi.useRealTimers();
  });

  it("pushes success and error toasts into the host", async () => {
    const host = mount(ToastHost, { attachTo: document.body });
    toast.success("项目已打开");
    toast.error("打开失败", { description: "路径无效" });
    await nextTick();

    expect(host.get('[data-testid="toast-success"]').text()).toContain("项目已打开");
    expect(host.get('[data-testid="toast-error"]').text()).toContain("打开失败");
    expect(host.get('[data-testid="toast-error"]').text()).toContain("路径无效");

    host.unmount();
  });

  it("auto-dismisses success toasts and keeps error sticky by default", async () => {
    vi.useFakeTimers();
    const host = mount(ToastHost, { attachTo: document.body });
    toast.success("短暂提示");
    toast.error("需要处理");
    await nextTick();

    expect(host.findAll("[data-toast-id]")).toHaveLength(2);
    vi.advanceTimersByTime(3100);
    await nextTick();

    const remaining = host.findAll("[data-toast-id]");
    expect(remaining).toHaveLength(1);
    expect(remaining[0].attributes("data-testid")).toBe("toast-error");

    host.unmount();
  });

  it("caps the stack at three toasts", async () => {
    const host = mount(ToastHost, { attachTo: document.body });
    toast.info("1");
    toast.info("2");
    toast.info("3");
    toast.info("4");
    await nextTick();
    expect(host.findAll("[data-toast-id]")).toHaveLength(3);
    host.unmount();
  });
});

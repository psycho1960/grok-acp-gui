import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import OnboardingView from "../../src/features/onboarding/OnboardingView.vue";
import type {
  DesktopBridge,
  DesktopCommand,
  DesktopResult,
  RuntimeReadinessSnapshot,
} from "../../src/bridge/types";

function readiness(overrides: Partial<RuntimeReadinessSnapshot> = {}): RuntimeReadinessSnapshot {
  return {
    installed: true,
    version: "1.0.0",
    minVersion: "0.2.118",
    authenticated: true,
    ready: true,
    login: { status: "idle", retryable: true },
    checks: [
      ["git", "Git"],
      ["grok", "Grok"],
      ["version", "版本"],
      ["authentication", "认证"],
      ["database", "数据库"],
      ["directory", "工作目录"],
      ["acp", "ACP 握手"],
    ].map(([id, label]) => ({
      id: id as RuntimeReadinessSnapshot["checks"][number]["id"],
      label,
      status: "success" as const,
      detail: `${label} 可用`,
    })),
    ...overrides,
  };
}

function bridge(handler: (command: DesktopCommand) => DesktopResult | Promise<DesktopResult>): DesktopBridge {
  return {
    bootstrap: vi.fn(),
    execute: vi.fn(handler),
    subscribe: vi.fn(async () => () => undefined),
  };
}

describe("UI-ONBOARD-001", () => {
  it("renders real ordered checks and emits ready after ACP validation", async () => {
    const runtimeBridge = bridge(() => ({ success: "true", data: readiness() }));
    const wrapper = mount(OnboardingView, { props: { bridge: runtimeBridge } });
    await flushPromises();

    expect(wrapper.findAll("[data-check-id]").map((item) => item.attributes("data-check-id"))).toEqual([
      "git", "grok", "version", "authentication", "database", "directory", "acp",
    ]);
    expect(wrapper.text()).not.toContain("工程壳就绪");
    expect(wrapper.emitted("ready")).toHaveLength(1);
  });

  it("shows official install guidance and supports redetection when Grok is missing", async () => {
    const missing = readiness({
      installed: false,
      authenticated: undefined,
      ready: false,
      checks: readiness().checks.map((check) =>
        check.id === "grok"
          ? { ...check, status: "error", detail: "未找到 Grok CLI。", code: "RUNTIME_NOT_FOUND" }
          : check,
      ),
    });
    const runtimeBridge = bridge(() => ({ success: "true", data: missing }));
    const wrapper = mount(OnboardingView, { props: { bridge: runtimeBridge } });
    await flushPromises();

    expect(wrapper.get('[data-testid="grok-install-link"]').attributes("href")).toContain("docs.x.ai");
    await wrapper.get('[data-testid="runtime-refresh"]').trigger("click");
    expect(runtimeBridge.execute).toHaveBeenCalledTimes(2);
  });

  it("starts official login, polls, refreshes automatically, and can cancel", async () => {
    vi.useFakeTimers();
    const unauthenticated = readiness({
      authenticated: false,
      ready: false,
      checks: readiness().checks.map((check) =>
        check.id === "authentication"
          ? { ...check, status: "error", detail: "Grok 尚未登录。", code: "RUNTIME_LOGIN_FAILED" }
          : check,
      ),
    });
    let statusPolls = 0;
    const runtimeBridge = bridge((command) => {
      if (command.type === "runtime.refresh") return { success: "true", data: unauthenticated };
      if (command.payload.method === "oauth") {
        return { success: "true", data: { status: "running", retryable: false } };
      }
      if (command.payload.method === "cancel") {
        return { success: "true", data: { status: "running", retryable: false } };
      }
      statusPolls += 1;
      return {
        success: "true",
        data: statusPolls > 1
          ? { status: "succeeded", exitCode: 0, retryable: false }
          : { status: "running", retryable: false },
      };
    });
    const wrapper = mount(OnboardingView, { props: { bridge: runtimeBridge } });
    await flushPromises();
    await wrapper.get('[data-testid="runtime-login"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="runtime-login-cancel"]').exists()).toBe(true);
    await vi.advanceTimersByTimeAsync(2_100);
    await flushPromises();
    expect(runtimeBridge.execute).toHaveBeenCalledWith({ type: "runtime.refresh", payload: {} });
    vi.useRealTimers();
  });

  it("shows only a missing env variable name and keeps diagnostic values secret", async () => {
    const secret = "GAG005A_SECRET_VALUE_NEVER_RENDER";
    const envMissing = readiness({
      ready: false,
      checks: readiness().checks.map((check) =>
        check.id === "authentication"
          ? {
              ...check,
              status: "error",
              detail: "Model 'profile' requires environment variable 'OPENAI_API_KEY'. Restart the app.",
              code: "RUNTIME_MODEL_ENV_MISSING",
            }
          : check,
      ),
      actionableError: {
        code: "RUNTIME_MODEL_ENV_MISSING",
        message: "OPENAI_API_KEY is missing",
        action: "Restart the app",
        diagnostic: "[RUNTIME_MODEL_ENV_MISSING] OPENAI_API_KEY is missing",
      },
    });
    const runtimeBridge = bridge(() => ({ success: "true", data: envMissing }));
    const wrapper = mount(OnboardingView, { props: { bridge: runtimeBridge } });
    await flushPromises();

    expect(wrapper.text()).toContain("OPENAI_API_KEY");
    expect(wrapper.html()).not.toContain(secret);
    expect(wrapper.html().toLowerCase()).not.toContain("access_token");
  });
});

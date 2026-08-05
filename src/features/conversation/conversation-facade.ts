// GAG-008: Bridge facade for conversation — turn.send / turn.cancel / subscribe.
// Never calls shell, git, or sqlite directly.

import type {
  DesktopBridge,
  DesktopResult,
  TaskId,
  TurnSendResult,
  TypedDesktopEvent,
  Unsubscribe,
} from "../../bridge/types";

export type ConversationFacadeEvent =
  | { kind: "desktop"; event: TypedDesktopEvent }
  | { kind: "bridge_error"; message: string };

export interface ConversationFacade {
  sendTurn(
    taskId: TaskId,
    message: string,
    attachments?: string[],
  ): Promise<DesktopResult<TurnSendResult | unknown>>;
  cancelTurn(taskId: TaskId): Promise<DesktopResult>;
  resumeSession(taskId: TaskId): Promise<DesktopResult>;
  openTask(taskId: TaskId): Promise<DesktopResult>;
  resolvePermission(payload: import("../../bridge/types").PermissionResolvePayload): Promise<DesktopResult>;
  resolvePlan(payload: import("../../bridge/types").PlanResolvePayload): Promise<DesktopResult>;
  subscribe(
    listener: (evt: ConversationFacadeEvent) => void,
  ): Promise<Unsubscribe>;
}

export function createConversationFacade(
  bridge: DesktopBridge,
): ConversationFacade {
  return {
    async sendTurn(taskId, message, attachments) {
      return bridge.execute({
        type: "turn.send",
        payload: { taskId, message, attachments },
      });
    },

    async cancelTurn(taskId) {
      return bridge.execute({
        type: "turn.cancel",
        payload: { taskId },
      });
    },

    async resumeSession(taskId) {
      return bridge.execute({
        type: "session.resume",
        payload: { taskId },
      });
    },

    async openTask(taskId) {
      return bridge.execute({
        type: "task.open",
        payload: { taskId },
      });
    },

    async resolvePermission(payload) {
      return bridge.execute({ type: "permission.resolve", payload });
    },

    async resolvePlan(payload) {
      return bridge.execute({ type: "plan.resolve", payload });
    },

    async subscribe(listener) {
      try {
        return await bridge.subscribe((event) => {
          listener({ kind: "desktop", event });
        });
      } catch (error) {
        listener({
          kind: "bridge_error",
          message:
            error instanceof Error ? error.message : "无法订阅会话事件",
        });
        return () => undefined;
      }
    },
  };
}

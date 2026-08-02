export const controlStates = ["default", "hover", "focus", "active", "disabled", "loading", "error"] as const;
export type ControlState = (typeof controlStates)[number];

export function isUnavailable(state: ControlState, disabled = false): boolean {
  return disabled || state === "disabled" || state === "loading";
}

import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";
import { BREAKPOINTS, mediaMaxWidth, mediaMinResolution } from "./breakpoints";

export type ResponsiveState = {
  width: Ref<number>;
  isDrawerMode: Ref<boolean>;
  isCompactNav: Ref<boolean>;
  isFixedLeft: Ref<boolean>;
  isPageZoomed: Ref<boolean>;
  isMdDown: Ref<boolean>;
  isSmDown: Ref<boolean>;
  isXsDown: Ref<boolean>;
};

/**
 * Shared layout breakpoints. Prefer this over ad-hoc matchMedia in features.
 * Safe to call once per component; queries are cheap and local.
 */
export function useResponsive(): ResponsiveState {
  const width = ref(typeof window !== "undefined" ? window.innerWidth : BREAKPOINTS.xl);
  const isDrawerMode = ref(false);
  const isCompactNav = ref(false);
  const isFixedLeft = ref(false);
  const isPageZoomed = ref(false);
  const isMdDown = ref(false);
  const isSmDown = ref(false);
  const isXsDown = ref(false);

  let drawerQuery: MediaQueryList | undefined;
  let compactQuery: MediaQueryList | undefined;
  let fixedLeftQuery: MediaQueryList | undefined;
  let mdQuery: MediaQueryList | undefined;
  let smQuery: MediaQueryList | undefined;
  let xsQuery: MediaQueryList | undefined;

  function sync(): void {
    width.value = window.innerWidth;
    isPageZoomed.value = (window.visualViewport?.scale ?? 1) >= 1.75;
    isDrawerMode.value = (drawerQuery?.matches ?? false) || isPageZoomed.value;
    isCompactNav.value = (compactQuery?.matches ?? false) || isPageZoomed.value;
    isFixedLeft.value = fixedLeftQuery?.matches ?? false;
    isMdDown.value = mdQuery?.matches ?? false;
    isSmDown.value = smQuery?.matches ?? false;
    isXsDown.value = xsQuery?.matches ?? false;
  }

  onMounted(() => {
    drawerQuery = window.matchMedia(
      `${mediaMaxWidth(BREAKPOINTS.xl)}, ${mediaMinResolution(1.75)}`,
    );
    compactQuery = window.matchMedia(
      `${mediaMaxWidth(BREAKPOINTS.compact)}, ${mediaMinResolution(1.75)}`,
    );
    fixedLeftQuery = window.matchMedia(mediaMaxWidth(BREAKPOINTS.lg));
    mdQuery = window.matchMedia(mediaMaxWidth(BREAKPOINTS.md));
    smQuery = window.matchMedia(mediaMaxWidth(BREAKPOINTS.sm));
    xsQuery = window.matchMedia(mediaMaxWidth(BREAKPOINTS.xs));
    sync();
    drawerQuery.addEventListener("change", sync);
    compactQuery.addEventListener("change", sync);
    fixedLeftQuery.addEventListener("change", sync);
    mdQuery.addEventListener("change", sync);
    smQuery.addEventListener("change", sync);
    xsQuery.addEventListener("change", sync);
    window.visualViewport?.addEventListener("resize", sync);
    window.addEventListener("resize", sync);
  });

  onBeforeUnmount(() => {
    drawerQuery?.removeEventListener("change", sync);
    compactQuery?.removeEventListener("change", sync);
    fixedLeftQuery?.removeEventListener("change", sync);
    mdQuery?.removeEventListener("change", sync);
    smQuery?.removeEventListener("change", sync);
    xsQuery?.removeEventListener("change", sync);
    window.visualViewport?.removeEventListener("resize", sync);
    window.removeEventListener("resize", sync);
  });

  return {
    width,
    isDrawerMode,
    isCompactNav,
    isFixedLeft,
    isPageZoomed,
    isMdDown,
    isSmDown,
    isXsDown,
  };
}

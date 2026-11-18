/**
 * Lets navigation step aside for the software keyboard. Android's native
 * insets shrink the whole WebView; mobile browsers may shrink only its visual
 * viewport. A lifetime height maximum mistakes rotation and split view for a
 * keyboard, so the layout fallback is armed by focusing/tapping an editor.
 */
export function watchKeyboard() {
  const viewport = window.visualViewport;
  const root = document.documentElement;
  const textInputTypes = new Set(["text", "search", "email", "url", "tel", "password", "number"]);
  const isEditor = (element: EventTarget | null) => {
    if (!(element instanceof HTMLElement) || element.inputMode === "none") return false;
    if (element instanceof HTMLTextAreaElement) return !element.disabled && !element.readOnly;
    if (element instanceof HTMLInputElement) {
      return textInputTypes.has(element.type) && !element.disabled && !element.readOnly;
    }
    return element.isContentEditable;
  };

  let width = window.innerWidth;
  let restingHeight = window.innerHeight;
  let layoutKeyboard = false;
  let openingUntil = 0;
  let blurTimer: number | undefined;

  const update = () => {
    const height = window.innerHeight;
    const typing = isEditor(document.activeElement);
    const geometryChanged = Math.abs(window.innerWidth - width) > 1;
    width = window.innerWidth;
    if (!typing || geometryChanged) {
      restingHeight = height;
      layoutKeyboard = false;
      openingUntil = 0;
    }

    // Ignore browser chrome and pinch zoom. Neither is a software keyboard.
    const unzoomed = !viewport || Math.abs(viewport.scale - 1) < 0.05;
    const threshold = Math.max(120, restingHeight * 0.15);
    const visualKeyboard = typing && unzoomed && height - (viewport?.height ?? height) > threshold;
    const touchDevice = navigator.maxTouchPoints > 0 || window.matchMedia("(any-pointer: coarse)").matches;
    const layoutGap = restingHeight - height;
    if (
      typing &&
      touchDevice &&
      unzoomed &&
      layoutGap > threshold &&
      (layoutKeyboard || performance.now() < openingUntil)
    ) {
      layoutKeyboard = true;
    } else {
      layoutKeyboard = false;
      // A settled window resize is the new baseline, even while a field keeps
      // focus. Preserve the pre-keyboard height only during its opening animation.
      if (height > restingHeight || performance.now() >= openingUntil) restingHeight = height;
    }

    const open = typing && (visualKeyboard || layoutKeyboard);
    root.toggleAttribute("data-keyboard", open);
    if (open) {
      const visibleHeight = unzoomed ? Math.min(height, viewport?.height ?? height) : height;
      root.style.setProperty("--app-height", `${visibleHeight}px`);
      root.style.setProperty("--app-offset-top", `${unzoomed ? Math.max(0, viewport?.offsetTop ?? 0) : 0}px`);
    } else {
      root.style.removeProperty("--app-height");
      root.style.removeProperty("--app-offset-top");
    }
  };

  const arm = () => {
    if (!layoutKeyboard) restingHeight = window.innerHeight;
    openingUntil = performance.now() + 1500;
  };
  const focus = () => {
    if (isEditor(document.activeElement)) arm();
    update();
  };
  const pointer = (event: PointerEvent) => {
    // Reopening a keyboard on an already-focused field emits no focusin.
    if (isEditor(event.target)) arm();
  };
  const blur = () => {
    window.clearTimeout(blurTimer);
    blurTimer = window.setTimeout(update, 50);
  };
  const rotate = () => {
    restingHeight = window.innerHeight;
    width = window.innerWidth;
    layoutKeyboard = false;
    openingUntil = 0;
    update();
  };

  window.addEventListener("resize", update);
  window.addEventListener("focusin", focus);
  window.addEventListener("focusout", blur);
  window.addEventListener("pointerdown", pointer);
  screen.orientation?.addEventListener("change", rotate);
  viewport?.addEventListener("resize", update);
  viewport?.addEventListener("scroll", update);
  update();

  return () => {
    window.clearTimeout(blurTimer);
    window.removeEventListener("resize", update);
    window.removeEventListener("focusin", focus);
    window.removeEventListener("focusout", blur);
    window.removeEventListener("pointerdown", pointer);
    screen.orientation?.removeEventListener("change", rotate);
    viewport?.removeEventListener("resize", update);
    viewport?.removeEventListener("scroll", update);
    root.removeAttribute("data-keyboard");
    root.style.removeProperty("--app-height");
    root.style.removeProperty("--app-offset-top");
  };
}

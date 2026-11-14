/**
 * Marks `<html data-keyboard="open">` while the on-screen keyboard is up.
 *
 * On Android the app is shrunk above the keyboard (see MainActivity.kt), so
 * the keyboard shows up here as the window getting much shorter while a text
 * field has focus. The tab bar then steps aside, so a
 * message box sits right on top of the keyboard instead of above it.
 */
export function watchKeyboard() {
  let tallest = window.innerHeight;
  const update = () => {
    tallest = Math.max(tallest, window.innerHeight);
    const field = document.activeElement;
    const typing =
      field instanceof HTMLTextAreaElement ||
      (field instanceof HTMLInputElement && !["checkbox", "radio", "range", "button"].includes(field.type));
    const open = typing && window.innerHeight < tallest * 0.8;
    document.documentElement.toggleAttribute("data-keyboard", open);
  };
  window.addEventListener("resize", update);
  window.addEventListener("focusin", update);
  window.addEventListener("focusout", () => setTimeout(update, 50));
  // A rotated screen has a new full height.
  screen.orientation?.addEventListener("change", () => {
    tallest = 0;
    setTimeout(update, 300);
  });
}

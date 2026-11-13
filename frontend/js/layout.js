//! Resizing of the two sidebars of the app shell (ticket 0070).

export const SIDEBAR_MIN_WIDTH = 180;
export const SIDEBAR_MAX_WIDTH = 480;
export const CHAT_MIN_WIDTH = 240;
export const CHAT_MAX_WIDTH = 640;

export const INITIAL_SIDEBAR_WIDTH = 260;
export const INITIAL_CHAT_WIDTH = 360;

let sidebarWidth = INITIAL_SIDEBAR_WIDTH;
let chatWidth = INITIAL_CHAT_WIDTH;
let appElement = null;
let drag = null;

// The template of the app shell grid with the given sidebar and AI chat widths;
// the editor takes the remaining space.
export function sidebarChatTemplate(sidebarWidth, chatWidth) {
  return `${sidebarWidth}px 1fr ${chatWidth}px`;
}

// Applies the sidebar and AI chat widths to the app shell, so both sidebars
// stay resizable (ticket 0070); the resize handles follow the applied widths.
export function applySidebarChatWidths(app, widthSidebar, widthChat) {
  if (!app) {
    return;
  }
  sidebarWidth = widthSidebar;
  chatWidth = widthChat;
  app.style.gridTemplateColumns = sidebarChatTemplate(sidebarWidth, chatWidth);
  const sidebarResize = app.querySelector?.("#sidebar-resize");
  if (sidebarResize) {
    sidebarResize.style.setProperty("left", `${sidebarWidth}px`);
  }
  const chatResize = app.querySelector?.("#chat-resize");
  if (chatResize) {
    chatResize.style.setProperty("right", `${chatWidth}px`);
  }
}

// The clamped value of one resize dimension.
export function clamp(value, min, max) {
  return Math.min(Math.max(value, min), max);
}

function moveDrag(event) {
  if (!drag) {
    return;
  }
  const deltaX = event.clientX - drag.startX;
  if (drag.handle === "sidebar") {
    sidebarWidth = clamp(drag.sidebarWidth + deltaX, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
  } else {
    chatWidth = clamp(drag.chatWidth - deltaX, CHAT_MIN_WIDTH, CHAT_MAX_WIDTH);
  }
  applySidebarChatWidths(appElement, sidebarWidth, chatWidth);
}

function endDrag() {
  drag = null;
  if (document.body) {
    document.body.style.userSelect = "";
  }
}

// Adjusts one sidebar by a keyboard step and re-applies the widths (ticket 0070).
function adjustByStep(handle, step) {
  if (handle === "sidebar") {
    sidebarWidth = clamp(sidebarWidth + step, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
  } else {
    chatWidth = clamp(chatWidth + step, CHAT_MIN_WIDTH, CHAT_MAX_WIDTH);
  }
  applySidebarChatWidths(appElement, sidebarWidth, chatWidth);
}

// Installs the drag-to-resize handles of the two sidebars of the app shell; the
// handle of the left sidebar resizes it, the handle of the right sidebar resizes
// the AI chat (ticket 0070).
export function installResizeHandles() {
  const app = document.querySelector("#app");
  const sidebarResize = document.querySelector("#sidebar-resize");
  const chatResize = document.querySelector("#chat-resize");
  if (!app || !sidebarResize || !chatResize) {
    return;
  }
  appElement = app;

  const startDrag = (handle) => (event) => {
    event.preventDefault();
    drag = { handle, startX: event.clientX, sidebarWidth: sidebarWidth, chatWidth: chatWidth };
    if (document.body) {
      document.body.style.userSelect = "none";
    }
    const onMove = (moveEvent) => moveDrag(moveEvent);
    const onEnd = () => {
      endDrag();
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onEnd);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onEnd);
  };

  const adjust = (handle) => (event) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") {
      return;
    }
    event.preventDefault();
    adjustByStep(handle, event.key === "ArrowRight" ? 1 : -1);
  };

  sidebarResize.addEventListener("pointerdown", startDrag("sidebar"));
  chatResize.addEventListener("pointerdown", startDrag("chat"));
  sidebarResize.addEventListener("keydown", adjust("sidebar"));
  chatResize.addEventListener("keydown", adjust("chat"));
  sidebarResize.setAttribute("tabindex", "0");
  chatResize.setAttribute("tabindex", "0");
}

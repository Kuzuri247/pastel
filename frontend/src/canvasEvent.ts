// Floating event card pinned over the canvas. Used for player-specific
// announcements (cleared, correct guess, host changed) that benefit from a
// visual that draws the eye away from the chat panel.

const HOST_ID = "canvasEventHost";
const DEFAULT_DURATION_MS = 2400;

export type CanvasEventKind = "info" | "celebrate";

export interface CanvasEventOpts {
  avatarHtml: string;
  message: string;
  kind?: CanvasEventKind;
  durationMs?: number;
}

function ensureHost(): HTMLElement {
  let host = document.getElementById(HOST_ID);
  if (!host) {
    host = document.createElement("div");
    host.id = HOST_ID;
    host.className = "canvas-event-host";
    // Anchor over the canvas surface. The canvas container is the closest
    // ancestor that lays out at the right size; we append to body and
    // position absolutely against it via CSS.
    document.body.appendChild(host);
  }
  return host;
}

export function showCanvasEvent(opts: CanvasEventOpts): void {
  const { avatarHtml, message, kind = "info", durationMs = DEFAULT_DURATION_MS } =
    opts;
  const host = ensureHost();
  const card = document.createElement("div");
  card.className = `canvas-event canvas-event--${kind}`;
  card.innerHTML = `
    <span class="canvas-event-avatar">${avatarHtml}</span>
    <span class="canvas-event-text"></span>
  `;
  card.querySelector<HTMLElement>(".canvas-event-text")!.textContent = message;
  host.appendChild(card);
  void card.offsetWidth;
  card.classList.add("canvas-event--in");

  const dismiss = () => {
    card.classList.remove("canvas-event--in");
    card.classList.add("canvas-event--out");
    card.addEventListener("transitionend", () => card.remove(), { once: true });
  };
  window.setTimeout(dismiss, durationMs);
}

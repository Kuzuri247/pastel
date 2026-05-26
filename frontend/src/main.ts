import { DrawingSurface } from "./canvas";
import { rgbToCss } from "./palette";
import { parseRoomCode, type Player, type ServerMsg } from "./proto";
import { loadInitialColor, loadInitialTool, mountToolbar } from "./toolbar";
import { Conn, type ConnState } from "./ws";

function pickRoomCode(): string {
  const params = new URLSearchParams(window.location.search);
  const fromUrl = params.get("room");
  if (fromUrl) return parseRoomCode(fromUrl);
  const generated = randomCode();
  const url = new URL(window.location.href);
  url.searchParams.set("room", generated);
  window.history.replaceState({}, "", url.toString());
  return generated;
}

function randomCode(): string {
  const alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
  let out = "";
  for (let i = 0; i < 6; i++) {
    out += alphabet[Math.floor(Math.random() * alphabet.length)];
  }
  return out;
}

function pickName(): string {
  const stored = window.localStorage.getItem("pastel.name");
  const prompt = stored
    ? `Hi ${stored}! Keep this name, or type a new one:`
    : "Pick a name";
  const reply = window.prompt(prompt, stored ?? "");
  // Cancel: keep stored if present, fall back to anon.
  if (reply === null) {
    if (stored) return stored;
    window.localStorage.setItem("pastel.name", "anon");
    return "anon";
  }
  const trimmed = reply.trim().slice(0, 32) || stored || "anon";
  window.localStorage.setItem("pastel.name", trimmed);
  return trimmed;
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!),
  );
}

const canvasEl = document.getElementById("canvas") as HTMLCanvasElement;
const statusEl = document.getElementById("status") as HTMLElement;
const playersEl = document.getElementById("players") as HTMLElement;
const toolbarEl = document.getElementById("toolbar") as HTMLElement;

const room = pickRoomCode();
const name = pickName();
document.title = `pastel · ${room}`;

const surface = new DrawingSurface(canvasEl);

const initialColor = loadInitialColor();
const initialTool = loadInitialTool();
surface.setColor(initialColor);
surface.setWidth(initialTool.width);

mountToolbar(toolbarEl, {
  onColor: (rgb) => surface.setColor(rgb),
  onTool: (tool) => surface.setWidth(tool.width),
  onClear: () => conn.send({ kind: "Game", action: { kind: "Clear" } }),
});

const players = new Map<number, Player>();
const playerColors = new Map<number, number>();

function renderPlayers(): void {
  const items = Array.from(players.values()).map((p) => {
    const color = rgbToCss(playerColors.get(p.id) ?? 0x76767c);
    return `<li><span class="swatch" style="background:${color}"></span>${escapeHtml(
      p.name,
    )}</li>`;
  });
  playersEl.innerHTML = `<h2>Room ${room}</h2><ul>${items.join("")}</ul>`;
}

renderPlayers();

const wsUrl = (() => {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.host}/ws/${room}`;
})();

function handleMessage(msg: ServerMsg): void {
  switch (msg.kind) {
    case "Welcome":
      surface.setYouId(msg.you);
      players.clear();
      playerColors.clear();
      for (const p of msg.snapshot.players) players.set(p.id, p);
      players.set(msg.you, { id: msg.you, name });
      for (const s of msg.snapshot.completed) playerColors.set(s.player, s.color);
      renderPlayers();
      surface.applySnapshot(msg);
      return;
    case "Presence":
      for (const p of msg.joined) players.set(p.id, p);
      for (const id of msg.left) {
        players.delete(id);
        playerColors.delete(id);
      }
      renderPlayers();
      return;
    case "Stroke":
      playerColors.set(msg.player, msg.color);
      surface.handleStrokeMessage(
        msg.player,
        msg.stroke_id,
        msg.origin,
        msg.color,
        msg.width,
        msg.points,
        msg.finished,
      );
      renderPlayers();
      return;
    case "Resume":
      for (const e of msg.events) handleMessage(e);
      return;
    case "Bye":
      statusEl.textContent = `disconnected: ${msg.reason.toLowerCase()}`;
      return;
    case "Game":
      if (msg.event.kind === "Cleared") surface.clear();
      return;
    case "Chat":
    case "Guess":
    case "Ping":
      return;
  }
}

function handleState(s: ConnState): void {
  switch (s.kind) {
    case "connecting":
      statusEl.textContent = "connecting…";
      return;
    case "open":
      statusEl.textContent = `room ${room} · ${name}`;
      return;
    case "reconnecting":
      statusEl.textContent = `reconnecting (attempt ${s.attempt})…`;
      return;
    case "closed":
      statusEl.textContent = `disconnected: ${s.reason}`;
      return;
  }
}

const conn = new Conn({
  url: wsUrl,
  hello: () => ({ kind: "Hello", hello: { room, name, resume_from: null } }),
  onMessage: handleMessage,
  onState: handleState,
});

surface.attachSender((msg) => conn.send(msg));

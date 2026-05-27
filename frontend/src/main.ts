import { CHAT_BUCKET_CAPACITY, CHAT_BUCKET_REFILL_PER_SEC, TokenBucket } from "./bucket";
import { DrawingSurface } from "./canvas";
import { mountChat, type ChatPanel } from "./chat";
import { applyScores, emptyState, type GamePhase, type GameState } from "./game";
import { mountGameUI } from "./gameUI";
import { showLanding } from "./landing";
import { rgbToCss } from "./palette";
import {
  parseRoomCode,
  type GameMode,
  type Player,
  type ServerMsg,
} from "./proto";
import { loadInitialColor, loadInitialTool, mountToolbar } from "./toolbar";
import { Conn, type ConnState } from "./ws";

// Show the landing screen if no room is in the URL. The landing form
// redirects to ?room=CODE&host=1&mode=MODE on submit.
const params = new URLSearchParams(window.location.search);
if (!params.has("room")) {
  showLanding();
} else {
  bootRoom();
}

function bootRoom(): void {

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
const chatEl = document.getElementById("chat") as HTMLElement;
const bannerEl = document.getElementById("banner") as HTMLElement;
const overlayEl = document.getElementById("gameOverlay") as HTMLElement;

const room = pickRoomCode();
const name = pickName();
document.title = `pastel · ${room}`;

const surface = new DrawingSurface(canvasEl);

const initialColor = loadInitialColor();
const initialTool = loadInitialTool();
surface.setColor(initialColor);
surface.setWidth(initialTool.width);

const players = new Map<number, Player>();
const playerColors = new Map<number, number>();
let youId: number | null = null;

const gameState: GameState = emptyState();

// The server's unicast (WordOptions, DrawerWord) is read by the connection
// task BEFORE the broadcast (WordPickStarted, RoundStart) thanks to the
// biased select on the server. So the drawer routinely sees the unicast
// before the broadcast. Stash whichever arrives first and merge it into the
// phase whenever the matching broadcast lands.
let pendingWordOptions: string[] | null = null;
let pendingDrawerWord: string | null = null;

function nameOf(id: number, fallback = "anon"): string {
  return players.get(id)?.name ?? fallback;
}

function colorOf(id: number): number {
  return playerColors.get(id) ?? 0x76767c;
}

function renderPlayers(): void {
  const youAreHost = youId !== null && youId === gameState.host;
  const items = Array.from(players.values()).map((p) => {
    const color = rgbToCss(colorOf(p.id));
    const score = gameState.scores.get(p.id);
    const scoreTag =
      score !== undefined ? `<span class="players-score">${score}</span>` : "";
    const youTag = p.id === youId ? '<span class="players-you">(you)</span>' : "";
    const hostTag =
      p.id === gameState.host ? '<span class="players-host">host</span>' : "";
    const kickBtn =
      youAreHost && p.id !== youId
        ? `<button class="players-kick" data-target="${p.id}" title="Remove ${escapeHtml(
            p.name,
          )} from the room" aria-label="Remove ${escapeHtml(p.name)}">×</button>`
        : "";
    return `<li>
      <span class="swatch" style="background:${color}"></span>
      <span class="players-name">${escapeHtml(p.name)}</span>
      ${youTag}${hostTag}${scoreTag}${kickBtn}
    </li>`;
  });
  playersEl.innerHTML = `
    <div class="players-head">
      <h2>Room <span class="room-code">${room}</span></h2>
      <button class="players-invite" type="button" title="Copy invite link">
        Invite
      </button>
    </div>
    <ul>${items.join("")}</ul>
  `;
  for (const btn of playersEl.querySelectorAll<HTMLButtonElement>(".players-kick")) {
    btn.addEventListener("click", () => {
      const target = Number(btn.dataset.target);
      if (Number.isNaN(target)) return;
      if (window.confirm(`Remove ${nameOf(target)} from the room?`)) {
        conn.send({ kind: "Game", action: { kind: "Kick", player: target } });
      }
    });
  }
  playersEl
    .querySelector<HTMLButtonElement>(".players-invite")
    ?.addEventListener("click", () => {
      void copyInviteLink();
    });
}

const chatBucket = new TokenBucket(CHAT_BUCKET_CAPACITY, CHAT_BUCKET_REFILL_PER_SEC);

const chat: ChatPanel = mountChat(chatEl, {
  onSend: (text) => {
    if (!chatBucket.tryTake()) return false;
    // During Drawing phase as a non-drawer, treat input as a guess. The
    // server treats both as text; this is just routing semantic.
    if (gameState.phase.kind === "Drawing" && gameState.phase.drawer !== youId) {
      conn.send({ kind: "Guess", text });
    } else {
      conn.send({ kind: "Chat", text });
    }
    return true;
  },
});

const gameUI = mountGameUI(overlayEl, {
  onStart: (mode: GameMode) => conn.send({ kind: "Game", action: { kind: "Start", mode } }),
  onPickWord: (index) =>
    conn.send({ kind: "Game", action: { kind: "PickWord", index } }),
  onRematch: () => {
    // Returning to Lobby is purely client-side until the server is told
    // to Start again. Show the mode picker.
    gameState.phase = { kind: "Lobby" };
    renderGameUI();
  },
});

mountToolbar(toolbarEl, {
  onColor: (rgb) => surface.setColor(rgb),
  onTool: (tool) => surface.setWidth(tool.width),
  onClear: () => conn.send({ kind: "Game", action: { kind: "Clear" } }),
});

renderPlayers();
renderGameUI();
startBannerTicker();

const wsUrl = (() => {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.host}/ws/${room}`;
})();

function renderGameUI(): void {
  gameUI.render(gameState.phase, {
    you: youId,
    host: gameState.host,
    playerCount: players.size,
    nameOf: (id) => nameOf(id),
    onCopyInvite: copyInviteLink,
  });
  updateBanner();
}

async function copyInviteLink(): Promise<void> {
  const url = window.location.href;
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(url);
    } else {
      // Fallback for non-secure contexts (e.g. plain http during dev).
      const ta = document.createElement("textarea");
      ta.value = url;
      ta.setAttribute("readonly", "");
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    chat.appendSystem("invite link copied to clipboard");
  } catch {
    chat.appendSystem("could not copy invite link, here it is: " + url);
  }
}

function updateBanner(): void {
  const phase = gameState.phase;
  if (phase.kind !== "Drawing" && phase.kind !== "ChoosingWord") {
    bannerEl.classList.add("banner--hidden");
    return;
  }
  bannerEl.classList.remove("banner--hidden");
  const text = gameUI.bannerText(phase) ?? "";
  const round = `Round ${phase.roundIndex + 1}/${phase.totalRounds}`;
  const isDrawing = phase.kind === "Drawing";
  const isDrawer = isDrawing && phase.drawer === youId;
  const hint = isDrawing && !isDrawer
    ? `<div class="banner-hint">${escapeHtml(nameOf(phase.drawer))} is drawing · you can scribble locally, only you see it</div>`
    : "";
  bannerEl.innerHTML = `
    <div class="banner-main">
      <div class="banner-round">${escapeHtml(round)}</div>
      <div class="banner-mask">${escapeHtml(text)}</div>
      <div class="banner-timer" id="bannerTimer">--</div>
    </div>
    ${hint}
  `;
}

function startBannerTicker(): void {
  function tick(): void {
    const timerEl = document.getElementById("bannerTimer");
    const phase = gameState.phase;
    if (timerEl) {
      const deadline =
        phase.kind === "Drawing" || phase.kind === "ChoosingWord"
          ? phase.deadline
          : null;
      if (deadline !== null) {
        const ms = Math.max(0, deadline - performance.now());
        const secs = Math.ceil(ms / 1000);
        timerEl.textContent = `${secs}s`;
        if (secs <= 10) {
          timerEl.classList.add("banner-timer--low");
        } else {
          timerEl.classList.remove("banner-timer--low");
        }
      }
    }
    requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);
}

function applyGameSnapshot(snap: import("./proto").GameSnapshot): void {
  gameState.host = snap.host;
  gameState.scores.clear();
  for (const [id, v] of snap.scores) gameState.scores.set(id, v);
  switch (snap.phase.kind) {
    case "Lobby":
      gameState.phase = { kind: "Lobby" };
      return;
    case "ChoosingWord":
      gameState.phase = {
        kind: "ChoosingWord",
        drawer: snap.phase.drawer,
        deadline: performance.now() + snap.phase.deadline_ms,
        roundIndex: snap.phase.round_index,
        totalRounds: snap.phase.total_rounds,
      };
      return;
    case "Drawing":
      gameState.phase = {
        kind: "Drawing",
        drawer: snap.phase.drawer,
        mask: snap.phase.mask,
        deadline: performance.now() + snap.phase.deadline_ms,
        durationMs: snap.phase.deadline_ms,
        roundIndex: snap.phase.round_index,
        totalRounds: snap.phase.total_rounds,
      };
      return;
    case "RoundEnd":
      gameState.phase = {
        kind: "RoundEnd",
        word: snap.phase.word,
        scores: snap.scores,
      };
      return;
    case "GameOver":
      gameState.phase = { kind: "GameOver", finalScores: snap.scores };
      return;
  }
}

function handleMessage(msg: ServerMsg): void {
  switch (msg.kind) {
    case "Welcome": {
      surface.setYouId(msg.you);
      youId = msg.you;
      players.clear();
      playerColors.clear();
      for (const p of msg.snapshot.players) players.set(p.id, p);
      players.set(msg.you, { id: msg.you, name });
      for (const s of msg.snapshot.completed) playerColors.set(s.player, s.color);
      applyGameSnapshot(msg.snapshot.game);
      renderPlayers();
      surface.applySnapshot(msg);
      chat.clear();
      for (const line of msg.snapshot.chat) {
        chat.appendMessage(
          nameOf(line.player),
          line.text,
          colorOf(line.player),
          line.player === youId,
        );
      }
      chat.appendSystem(`joined room ${room}`);
      renderGameUI();
      return;
    }
    case "Presence": {
      for (const p of msg.joined) {
        players.set(p.id, p);
        if (p.id !== youId) chat.appendSystem(`${p.name} joined`);
      }
      for (const id of msg.left) {
        const who = nameOf(id);
        players.delete(id);
        playerColors.delete(id);
        chat.appendSystem(`${who} left`);
      }
      renderPlayers();
      return;
    }
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
    case "Chat":
      chat.appendMessage(
        nameOf(msg.player),
        msg.text,
        colorOf(msg.player),
        msg.player === youId,
      );
      return;
    case "Guess":
      if (msg.guess === "Correct") {
        chat.appendCorrectGuess(nameOf(msg.player), colorOf(msg.player));
      }
      return;
    case "Resume":
      for (const e of msg.events) handleMessage(e);
      return;
    case "Bye":
      statusEl.textContent = `disconnected: ${msg.reason.toLowerCase()}`;
      chat.appendSystem(`disconnected: ${msg.reason.toLowerCase()}`);
      return;
    case "Game":
      handleGameEvent(msg.event);
      return;
    case "WordOptions": {
      pendingWordOptions = msg.words;
      if (gameState.phase.kind === "ChoosingWord") {
        gameState.phase = { ...gameState.phase, myOptions: msg.words };
        renderGameUI();
      }
      return;
    }
    case "DrawerWord": {
      pendingDrawerWord = msg.word;
      if (gameState.phase.kind === "Drawing") {
        gameState.phase = { ...gameState.phase, myWord: msg.word };
        renderGameUI();
      }
      return;
    }
    case "Ping":
      return;
  }
}

function handleGameEvent(event: Extract<ServerMsg, { kind: "Game" }>["event"]): void {
  switch (event.kind) {
    case "Cleared":
      surface.clear();
      chat.appendSystem(`${nameOf(event.by)} cleared the canvas`);
      return;
    case "WordPickStarted": {
      const deadline = performance.now() + event.deadline_ms;
      gameState.phase = {
        kind: "ChoosingWord",
        drawer: event.drawer,
        deadline,
        roundIndex: event.round_index,
        totalRounds: event.total_rounds,
        myOptions:
          event.drawer === youId && pendingWordOptions
            ? pendingWordOptions
            : undefined,
      };
      // New round: any stashed DrawerWord from a previous round is stale.
      pendingDrawerWord = null;
      pendingWordOptions = null;
      surface.clear();
      chat.appendSystem(
        `Round ${event.round_index + 1}/${event.total_rounds}: ${nameOf(event.drawer)} is picking a word`,
      );
      renderGameUI();
      return;
    }
    case "RoundStart": {
      const deadline = performance.now() + event.duration_ms;
      gameState.phase = {
        kind: "Drawing",
        drawer: event.drawer,
        mask: event.word_mask,
        myWord:
          event.drawer === youId && pendingDrawerWord
            ? pendingDrawerWord
            : undefined,
        deadline,
        durationMs: event.duration_ms,
        roundIndex: event.round_index,
        totalRounds: event.total_rounds,
      };
      pendingDrawerWord = null;
      pendingWordOptions = null;
      renderGameUI();
      return;
    }
    case "HintReveal":
      if (gameState.phase.kind === "Drawing") {
        gameState.phase = { ...gameState.phase, mask: event.mask };
        renderGameUI();
      }
      return;
    case "RoundEnd":
      applyScores(gameState, event.scores);
      gameState.phase = {
        kind: "RoundEnd",
        word: event.word,
        scores: event.scores,
      };
      chat.appendSystem(`Word was "${event.word}"`);
      renderPlayers();
      renderGameUI();
      return;
    case "GameOver":
      applyScores(gameState, event.final_scores);
      gameState.phase = {
        kind: "GameOver",
        finalScores: event.final_scores,
      };
      renderPlayers();
      renderGameUI();
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

  // Avoid an unused-import lint when game.ts re-exports types we only use as
  // types at call sites.
  void (null as unknown as GamePhase);
}

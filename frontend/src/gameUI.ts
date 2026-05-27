// UI for the game loop overlays: mode picker, word pick, drawing banner
// (mask + timer + round counter), round-end reveal, game-over podium.

import { MODE_OPTIONS, type GamePhase } from "./game";
import type { GameMode } from "./proto";

export interface GameUIHandlers {
  onStart: (mode: GameMode) => void;
  onPickWord: (index: number) => void;
  onRematch: () => void;
}

export interface RenderContext {
  you: number | null;
  host: number | null;
  playerCount: number;
  nameOf: (id: number) => string;
  onCopyInvite: () => void;
}

export interface GameUI {
  render(phase: GamePhase, ctx: RenderContext): void;
  /// Returns the current word/mask to render in the top banner. null if no
  /// game is running.
  bannerText(phase: GamePhase): string | null;
}

export function mountGameUI(root: HTMLElement, handlers: GameUIHandlers): GameUI {
  root.classList.add("game-overlay");
  root.innerHTML = "";

  function clear(): void {
    root.innerHTML = "";
    root.classList.remove("game-overlay--visible");
  }

  function visible(): void {
    root.classList.add("game-overlay--visible");
  }

  function renderLobby(ctx: RenderContext): void {
    visible();
    const isHost = ctx.you !== null && ctx.you === ctx.host;
    if (!isHost) {
      const hostName = ctx.host !== null ? ctx.nameOf(ctx.host) : "the host";
      root.innerHTML = `
        <div class="overlay-card">
          <h2>Waiting for ${escapeHtml(hostName)}</h2>
          <p class="overlay-hint">${escapeHtml(hostName)} picks the mode and starts the game.</p>
          <button type="button" class="invite-primary">Copy invite link</button>
        </div>
      `;
      wireInvite(ctx);
      return;
    }
    if (ctx.playerCount < 2) {
      root.innerHTML = `
        <div class="overlay-card">
          <h2>You're alone in here</h2>
          <p class="overlay-hint">
            At least 2 players needed to start. Did you invite anyone?
          </p>
          <button type="button" class="invite-primary">Copy invite link</button>
        </div>
      `;
      wireInvite(ctx);
      return;
    }
    const options = MODE_OPTIONS.map(
      (m) => `
      <button type="button" class="mode-card" data-mode="${m.id}">
        <span class="mode-label">${m.label}</span>
        <span class="mode-meta">${m.rounds} rounds · ${m.wordChoices} words</span>
      </button>`,
    ).join("");
    root.innerHTML = `
      <div class="overlay-card">
        <h2>Pick a mode</h2>
        <div class="mode-grid">${options}</div>
        <p class="overlay-hint">Every player draws once per round. You're the host.</p>
        <button type="button" class="invite-secondary">Copy invite link</button>
      </div>
    `;
    for (const btn of root.querySelectorAll<HTMLButtonElement>(".mode-card")) {
      btn.addEventListener("click", () => {
        const mode = btn.dataset.mode as GameMode;
        handlers.onStart(mode);
      });
    }
    wireInvite(ctx);
  }

  function wireInvite(ctx: RenderContext): void {
    for (const btn of root.querySelectorAll<HTMLButtonElement>(
      ".invite-primary, .invite-secondary",
    )) {
      btn.addEventListener("click", () => {
        ctx.onCopyInvite();
        const original = btn.textContent;
        btn.textContent = "Link copied";
        btn.disabled = true;
        setTimeout(() => {
          btn.textContent = original;
          btn.disabled = false;
        }, 1500);
      });
    }
  }

  function renderChoosing(
    phase: Extract<GamePhase, { kind: "ChoosingWord" }>,
    ctx: RenderContext,
  ): void {
    const you = ctx.you;
    const nameOf = ctx.nameOf;
    visible();
    if (phase.drawer === you && phase.myOptions) {
      const cards = phase.myOptions
        .map(
          (w, i) => `
            <button type="button" class="word-pick-card" data-index="${i}">
              <span class="word-pick-num">${String(i + 1).padStart(2, "0")}</span>
              <span class="word-pick-word">${escapeHtml(w)}</span>
            </button>`,
        )
        .join("");
      root.innerHTML = `
        <div class="overlay-card overlay-card--wide">
          <div class="word-pick-head">
            <span class="word-pick-eyebrow">Round ${phase.roundIndex + 1} of ${phase.totalRounds} · your turn</span>
            <h2>Pick a word</h2>
          </div>
          <div class="word-pick-grid">${cards}</div>
          <p class="overlay-hint">Auto-picks the first if you take too long.</p>
          <button type="button" class="invite-secondary">Copy invite link</button>
        </div>
      `;
      for (const btn of root.querySelectorAll<HTMLButtonElement>(".word-pick-card")) {
        btn.addEventListener("click", () => {
          const idx = Number(btn.dataset.index);
          handlers.onPickWord(idx);
        });
      }
    } else {
      root.innerHTML = `
        <div class="overlay-card">
          <div class="word-pick-head">
            <span class="word-pick-eyebrow">Round ${phase.roundIndex + 1} of ${phase.totalRounds}</span>
            <h2>${escapeHtml(nameOf(phase.drawer))} is picking a word</h2>
          </div>
          <p class="overlay-hint">Hang tight.</p>
          <button type="button" class="invite-secondary">Copy invite link</button>
        </div>
      `;
    }
    wireInvite(ctx);
  }

  function renderRoundEnd(
    phase: Extract<GamePhase, { kind: "RoundEnd" }>,
    nameOf: (id: number) => string,
  ): void {
    visible();
    const rows = phase.scores
      .map(
        ([id, score]) =>
          `<li><span>${escapeHtml(nameOf(id))}</span><span>${score}</span></li>`,
      )
      .join("");
    root.innerHTML = `
      <div class="overlay-card">
        <h2>The word was <em>${escapeHtml(phase.word)}</em></h2>
        <ul class="score-list">${rows}</ul>
      </div>
    `;
  }

  function renderGameOver(
    phase: Extract<GamePhase, { kind: "GameOver" }>,
    ctx: RenderContext,
  ): void {
    visible();
    const top = phase.finalScores.slice(0, 3);
    const podium = top
      .map(
        ([id, score], i) =>
          `<li class="podium-row podium-${i + 1}">
             <span class="podium-rank">#${i + 1}</span>
             <span class="podium-name">${escapeHtml(ctx.nameOf(id))}</span>
             <span class="podium-score">${score}</span>
           </li>`,
      )
      .join("");
    // If the game ended because everyone else bailed, say so plainly. The
    // server doesn't send a reason, but we can infer from the room state.
    const abandoned = ctx.playerCount < 2;
    const heading = abandoned ? "No one left to play with" : "Game over";
    const subtext = abandoned
      ? '<p class="overlay-hint">Everyone else left the room. Here are the points so far.</p>'
      : "";
    root.innerHTML = `
      <div class="overlay-card">
        <h2>${heading}</h2>
        ${subtext}
        <ul class="podium">${podium}</ul>
        <button type="button" class="rematch-btn">Play again</button>
      </div>
    `;
    root
      .querySelector<HTMLButtonElement>(".rematch-btn")
      ?.addEventListener("click", handlers.onRematch);
  }

  function render(phase: GamePhase, ctx: RenderContext): void {
    switch (phase.kind) {
      case "Lobby":
        renderLobby(ctx);
        break;
      case "ChoosingWord":
        renderChoosing(phase, ctx);
        break;
      case "RoundEnd":
        renderRoundEnd(phase, ctx.nameOf);
        break;
      case "GameOver":
        renderGameOver(phase, ctx);
        break;
      case "Drawing":
        clear();
        break;
    }
  }

  function bannerText(phase: GamePhase): string | null {
    switch (phase.kind) {
      case "Drawing":
        return phase.myWord ?? phase.mask;
      case "ChoosingWord":
        return null;
      default:
        return null;
    }
  }

  return { render, bannerText };
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
}

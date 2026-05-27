// UI for the game loop overlays: mode picker, word pick, drawing banner
// (mask + timer + round counter), round-end reveal, game-over podium.

import type { GamePhase } from "./game";

export interface GameUIHandlers {
  onStart: () => void;
  onPickWord: (index: number) => void;
  onRematch: () => void;
}

export interface RenderContext {
  you: number | null;
  host: number | null;
  playerCount: number;
  nameOf: (id: number) => string;
  avatarOf: (id: number) => string;
  modeBadge: string;
  playerAvatars: { id: number; name: string; avatarHtml: string }[];
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
    const canStart = isHost && ctx.playerCount >= 2;
    const avatarChips = ctx.playerAvatars
      .map(
        (p) => `<div class="lobby-player" title="${escapeHtml(p.name)}">
          <span class="lobby-player-avatar">${p.avatarHtml}</span>
          <span class="lobby-player-name">${escapeHtml(p.name)}</span>
        </div>`,
      )
      .join("");
    const hostName = ctx.host !== null ? ctx.nameOf(ctx.host) : "the host";
    const startSection = isHost
      ? `<button type="button" class="lobby-start" ${canStart ? "" : "disabled"}>
           Start game
         </button>
         ${
           ctx.playerCount < 2
             ? '<p class="lobby-hint">Waiting for at least 2 players to join</p>'
             : `<p class="lobby-hint">${ctx.playerCount} player${ctx.playerCount !== 1 ? "s" : ""} ready</p>`
         }`
      : `<p class="lobby-waiting">Waiting for <strong>${escapeHtml(hostName)}</strong> to start</p>`;

    root.innerHTML = `
      <div class="overlay-card overlay-card--lobby">
        <div class="lobby-head">
          <h2>Lobby</h2>
          <span class="lobby-mode-badge">${escapeHtml(ctx.modeBadge)}</span>
        </div>
        <div class="lobby-players">${avatarChips}</div>
        <div class="lobby-actions">
          ${startSection}
        </div>
        <div class="lobby-invite">
          <button type="button" class="invite-secondary">Copy invite link</button>
        </div>
      </div>
    `;
    root
      .querySelector<HTMLButtonElement>(".lobby-start")
      ?.addEventListener("click", () => handlers.onStart());
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
    ctx: RenderContext,
  ): void {
    visible();
    const rows = phase.scores
      .map(
        ([id, score]) =>
          `<li>
             <span class="score-avatar">${ctx.avatarOf(id)}</span>
             <span class="score-name">${escapeHtml(ctx.nameOf(id))}</span>
             <span class="score-points">${score}</span>
           </li>`,
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
             <span class="podium-avatar">${ctx.avatarOf(id)}</span>
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
        renderRoundEnd(phase, ctx);
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

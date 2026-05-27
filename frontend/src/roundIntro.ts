// Round-start intro card. Slides down over the canvas when a new round
// kicks off, calls out "Round N of M" + the drawer, and shows the current
// cumulative scoreboard. Auto-dismisses so play resumes quickly.

const HOST_ID = "roundIntroHost";
const HOLD_MS = 1700;

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
}

export interface RoundIntroOpts {
  roundIndex: number;
  totalRounds: number;
  drawerName: string;
  drawerAvatarHtml: string;
  scores: { id: number; name: string; avatarHtml: string; points: number }[];
  holdMs?: number;
}

function ensureHost(): HTMLElement {
  let host = document.getElementById(HOST_ID);
  if (!host) {
    host = document.createElement("div");
    host.id = HOST_ID;
    host.className = "round-intro-host";
    document.body.appendChild(host);
  }
  return host;
}

export function showRoundIntro(opts: RoundIntroOpts): void {
  const host = ensureHost();
  // Only one intro at a time. If a previous round's intro is still on
  // screen (extremely fast game), drop it so we don't stack.
  host.innerHTML = "";

  const rows = opts.scores
    .map(
      (s, i) => `
        <li class="round-intro-row" style="--row-i: ${i};">
          <span class="round-intro-rank">${i + 1}</span>
          <span class="round-intro-row-avatar">${s.avatarHtml}</span>
          <span class="round-intro-row-name">${escapeHtml(s.name)}</span>
          <span class="round-intro-row-score">${s.points}</span>
        </li>
      `,
    )
    .join("");

  const card = document.createElement("div");
  card.className = "round-intro";
  card.innerHTML = `
    <div class="round-intro-header">
      <span class="round-intro-eyebrow">
        Round ${opts.roundIndex + 1} of ${opts.totalRounds}
      </span>
      <div class="round-intro-drawer">
        <span class="round-intro-drawer-avatar">${opts.drawerAvatarHtml}</span>
        <div class="round-intro-drawer-text">
          <span class="round-intro-drawer-name">${escapeHtml(opts.drawerName)}</span>
          <span class="round-intro-drawer-sub">is up to draw</span>
        </div>
      </div>
    </div>
    ${
      rows
        ? `<ol class="round-intro-scores">${rows}</ol>`
        : '<div class="round-intro-empty">First round. Let\'s see what you\'ve got!</div>'
    }
  `;
  host.appendChild(card);
  void card.offsetWidth;
  card.classList.add("round-intro--in");

  const hold = opts.holdMs ?? HOLD_MS;
  window.setTimeout(() => {
    card.classList.remove("round-intro--in");
    card.classList.add("round-intro--out");
    card.addEventListener("transitionend", () => card.remove(), { once: true });
  }, hold);
}

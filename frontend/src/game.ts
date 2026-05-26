// Client-side game state tracker. The server is authoritative; this is just
// a view model that derives "what to render" from the stream of ServerMsg.

import type { GameMode } from "./proto";

export type GamePhase =
  | { kind: "Lobby" }
  | {
      kind: "ChoosingWord";
      drawer: number;
      deadline: number; // performance.now() ms
      roundIndex: number;
      totalRounds: number;
      // Only set if you are the drawer.
      myOptions?: string[];
    }
  | {
      kind: "Drawing";
      drawer: number;
      mask: string;
      // Only set if you are the drawer.
      myWord?: string;
      deadline: number; // performance.now() ms when round ends
      durationMs: number;
      roundIndex: number;
      totalRounds: number;
    }
  | { kind: "RoundEnd"; word: string; scores: [number, number][] }
  | { kind: "GameOver"; finalScores: [number, number][] };

export interface GameState {
  phase: GamePhase;
  scores: Map<number, number>;
  host: number | null;
}

export function emptyState(): GameState {
  return {
    phase: { kind: "Lobby" },
    scores: new Map(),
    host: null,
  };
}

// Score totals come from RoundEnd / GameOver events as cumulative
// (player, total) pairs. Replace the whole map each time.
export function applyScores(s: GameState, scores: [number, number][]): void {
  s.scores.clear();
  for (const [id, v] of scores) s.scores.set(id, v);
}

export function isDrawer(state: GameState, you: number | null): boolean {
  if (you === null) return false;
  return (
    (state.phase.kind === "ChoosingWord" && state.phase.drawer === you) ||
    (state.phase.kind === "Drawing" && state.phase.drawer === you)
  );
}

export interface ModeOption {
  id: GameMode;
  label: string;
  rounds: number;
  wordChoices: number;
}

export const MODE_OPTIONS: ModeOption[] = [
  { id: "Sprint", label: "Sprint", rounds: 3, wordChoices: 7 },
  { id: "Standard", label: "Standard", rounds: 5, wordChoices: 5 },
  { id: "Marathon", label: "Marathon", rounds: 7, wordChoices: 3 },
];

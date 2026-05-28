// Background lofi loop + event sound effects, all procedural via Tone.js.
// No audio files bundled. Defaults to OFF; user toggles via header button.
// State persisted in localStorage so the choice survives reloads.

import * as Tone from "tone";

const BG_KEY = "pastel.bg";
const SFX_KEY = "pastel.sfx";

let bgEnabled = false;
let sfxEnabled = false;
let started = false;
let toneStarted = false;
let bgPad: Tone.PolySynth | null = null;
let bgBass: Tone.Synth | null = null;
let bgHat: Tone.NoiseSynth | null = null;
let bgBell: Tone.Synth | null = null;
let bgKick: Tone.MembraneSynth | null = null;
let bgVinyl: Tone.Noise | null = null;

// Jazz-flavored chord progression (Cmaj9 -> Am11 -> Dm9 -> G13).
// Each entry: pad voicing (mid-high) + bass root for that bar.
const PROGRESSION: { pad: string[]; bass: string }[] = [
  { pad: ["E3", "G3", "B3", "D4"], bass: "C2" },
  { pad: ["E3", "G3", "C4", "D4"], bass: "A1" },
  { pad: ["F3", "A3", "C4", "E4"], bass: "D2" },
  { pad: ["F3", "A3", "B3", "E4"], bass: "G1" },
];

// Sparse pentatonic melody phrases (one per chord). Notes + offsets in beats.
const MELODY: { note: string; beat: number }[][] = [
  [{ note: "E5", beat: 0.5 }, { note: "G5", beat: 1.5 }, { note: "D5", beat: 3 }],
  [{ note: "C5", beat: 1 }, { note: "E5", beat: 2.5 }],
  [{ note: "F5", beat: 0.5 }, { note: "A5", beat: 2 }, { note: "E5", beat: 3.5 }],
  [{ note: "D5", beat: 1 }, { note: "G5", beat: 3 }],
];

function ensureStarted(): void {
  if (started) return;
  started = true;

  // Master chain: lowpass + warm reverb so everything melts together.
  const reverb = new Tone.Reverb({ decay: 5, wet: 0.35 }).toDestination();
  const warmth = new Tone.Filter(2200, "lowpass").connect(reverb);

  // Hazy pad. FMSynth gives a softer, foggier feel than plain sine.
  bgPad = new Tone.PolySynth({
    voice: Tone.FMSynth,
    options: {
      harmonicity: 1.5,
      modulationIndex: 3,
      oscillator: { type: "sine" },
      envelope: { attack: 1.8, decay: 0.8, sustain: 0.7, release: 3.5 },
      modulation: { type: "triangle" },
      modulationEnvelope: { attack: 1.5, decay: 0.5, sustain: 0.4, release: 2.5 },
    },
  }).connect(warmth);
  bgPad.volume.value = -18;

  // Sub bass: round, low.
  bgBass = new Tone.Synth({
    oscillator: { type: "sine" },
    envelope: { attack: 0.04, decay: 0.6, sustain: 0.4, release: 1.2 },
    volume: -14,
  }).connect(warmth);

  // Soft bell for the sparse melody.
  bgBell = new Tone.Synth({
    oscillator: { type: "triangle" },
    envelope: { attack: 0.01, decay: 0.6, sustain: 0.1, release: 0.8 },
    volume: -20,
  }).connect(warmth);

  // Subtle kick on beat 1 of each bar.
  bgKick = new Tone.MembraneSynth({
    pitchDecay: 0.05,
    octaves: 4,
    envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.4 },
    volume: -16,
  }).connect(warmth);

  // Filtered hi-hat ticks on off-beats.
  bgHat = new Tone.NoiseSynth({
    noise: { type: "pink" },
    envelope: { attack: 0.005, decay: 0.05, sustain: 0, release: 0.05 },
    volume: -32,
  }).connect(warmth);

  // Vinyl crackle: pink noise behind everything, very quiet, no envelope.
  bgVinyl = new Tone.Noise({ type: "pink", volume: -36 });
  const crackleFilter = new Tone.Filter(900, "highpass").toDestination();
  bgVinyl.connect(crackleFilter);
  bgVinyl.start();

  // One bar = 4 beats. Progression = 4 bars then loops.
  let bar = 0;
  new Tone.Loop((time) => {
    const chord = PROGRESSION[bar % PROGRESSION.length];
    // Pad sustains across the bar
    bgPad?.triggerAttackRelease(chord.pad, "1n", time, 0.9);
    // Bass on beat 1 and a softer hit on beat 3
    bgBass?.triggerAttackRelease(chord.bass, "4n", time, 0.9);
    bgBass?.triggerAttackRelease(
      chord.bass,
      "8n",
      time + Tone.Time("2n").toSeconds(),
      0.5,
    );
    // Kick on beat 1, lighter ghost on beat 3
    bgKick?.triggerAttackRelease("C1", "8n", time, 0.8);
    bgKick?.triggerAttackRelease("C1", "16n", time + Tone.Time("2n").toSeconds(), 0.3);
    // Hats: off-beats with slight swing
    const beat = Tone.Time("4n").toSeconds();
    bgHat?.triggerAttackRelease("32n", time + beat * 0.5);
    bgHat?.triggerAttackRelease("32n", time + beat * 1.5);
    bgHat?.triggerAttackRelease("32n", time + beat * 2.5);
    bgHat?.triggerAttackRelease("32n", time + beat * 3.5);
    // Sparse melody, only sometimes (60% of bars) to keep it breathing
    if (Math.random() < 0.6) {
      const phrase = MELODY[bar % MELODY.length];
      for (const { note, beat: b } of phrase) {
        const t = time + b * Tone.Time("4n").toSeconds();
        bgBell?.triggerAttackRelease(note, "8n", t, 0.5);
      }
    }
    bar++;
  }, "1m").start(0);

  Tone.Transport.bpm.value = 68;
  Tone.Transport.swing = 0.18;
  Tone.Transport.swingSubdivision = "8n";
}

async function ensureToneStarted(): Promise<void> {
  if (toneStarted) return;
  await Tone.start();
  toneStarted = true;
}

export function isBgEnabled(): boolean {
  return bgEnabled;
}

export function isSfxEnabled(): boolean {
  return sfxEnabled;
}

export async function enableBg(): Promise<void> {
  await ensureToneStarted();
  ensureStarted();
  if (Tone.Transport.state !== "started") {
    Tone.Transport.start();
  }
  bgEnabled = true;
  window.localStorage.setItem(BG_KEY, "1");
}

export function disableBg(): void {
  if (Tone.Transport.state === "started") {
    Tone.Transport.pause();
  }
  bgEnabled = false;
  window.localStorage.setItem(BG_KEY, "0");
}

export async function toggleBg(): Promise<boolean> {
  if (bgEnabled) disableBg();
  else await enableBg();
  return bgEnabled;
}

export async function enableSfx(): Promise<void> {
  await ensureToneStarted();
  sfxEnabled = true;
  window.localStorage.setItem(SFX_KEY, "1");
}

export function disableSfx(): void {
  sfxEnabled = false;
  window.localStorage.setItem(SFX_KEY, "0");
}

export async function toggleSfx(): Promise<boolean> {
  if (sfxEnabled) disableSfx();
  else await enableSfx();
  return sfxEnabled;
}

export function loadBgPreference(): boolean {
  // Default to on if the user has never set a preference
  return window.localStorage.getItem(BG_KEY) !== "0";
}

export function loadSfxPreference(): boolean {
  return window.localStorage.getItem(SFX_KEY) !== "0";
}

// One-shot event sounds. All gated on `enabled` so they're silent if audio is off.

function playMelody(notes: { note: string; time: number; dur: string }[]): void {
  if (!sfxEnabled) return;
  ensureStarted();
  const reverb = new Tone.Reverb({ decay: 2, wet: 0.3 }).toDestination();
  const synth = new Tone.Synth({
    oscillator: { type: "sine" },
    envelope: { attack: 0.02, decay: 0.2, sustain: 0.3, release: 0.6 },
    volume: -22,
  }).connect(reverb);
  const now = Tone.now();
  for (const { note, time, dur } of notes) {
    synth.triggerAttackRelease(note, dur, now + time);
  }
  const totalDur = notes.reduce((acc, n) => Math.max(acc, n.time + 1.0), 0);
  window.setTimeout(() => {
    synth.dispose();
    reverb.dispose();
  }, (totalDur + 1) * 1000);
}

export function playCorrect(): void {
  playMelody([
    { note: "C5", time: 0, dur: "16n" },
    { note: "E5", time: 0.1, dur: "16n" },
    { note: "G5", time: 0.2, dur: "8n" },
  ]);
}

export function playRoundStart(): void {
  playMelody([
    { note: "G4", time: 0, dur: "16n" },
    { note: "C5", time: 0.12, dur: "8n" },
  ]);
}

export function playRoundEnd(): void {
  playMelody([
    { note: "E5", time: 0, dur: "16n" },
    { note: "C5", time: 0.1, dur: "16n" },
    { note: "G4", time: 0.2, dur: "4n" },
  ]);
}

export function playJoin(): void {
  playMelody([
    { note: "E4", time: 0, dur: "16n" },
    { note: "A4", time: 0.08, dur: "16n" },
  ]);
}

export function playGameOver(): void {
  playMelody([
    { note: "C5", time: 0, dur: "16n" },
    { note: "E5", time: 0.1, dur: "16n" },
    { note: "G5", time: 0.2, dur: "16n" },
    { note: "C6", time: 0.3, dur: "4n" },
  ]);
}

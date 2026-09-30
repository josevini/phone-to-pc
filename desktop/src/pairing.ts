import type { Invite, PairingEvent, Qr } from "./status.ts";

/** Where a pairing started from the window is. */
export type PairState =
  /** Showing this device's code; `asking` is a device that wants its code compared. */
  | { kind: "code"; invite: Invite; expiresAt: number; asking: { name: string; code: string } | null }
  /** Entering another device's pairing link, or `busy` pairing with it. */
  | { kind: "link"; busy: boolean }
  | { kind: "paired"; name: string }
  /** The code expired before a device paired. */
  | { kind: "expired" }
  /** `retry` is where "Try again" goes. */
  | { kind: "failed"; reason: string; retry: "code" | "link" };

export function showingCode(invite: Invite, nowMs: number): PairState {
  return { kind: "code", invite, expiresAt: nowMs + invite.expires_in_s * 1000, asking: null };
}

export function onPairingEvent(state: PairState, event: PairingEvent): PairState {
  if (state.kind !== "code" && state.kind !== "link") return state;
  switch (event.type) {
    case "code":
      return state.kind === "code" ? { ...state, asking: { name: event.name, code: event.code } } : state;
    case "paired":
      return { kind: "paired", name: event.name };
    case "ended":
      return state.kind === "code" ? { kind: "expired" } : state;
    case "failed":
      return { kind: "failed", reason: event.reason, retry: state.kind };
  }
}

/** The user answered the device that asked to compare codes. */
export function answered(state: PairState): PairState {
  return state.kind === "code" ? { ...state, asking: null } : state;
}

/** `msLeft` as minutes and seconds, rounded up: "1:59". */
export function countdown(msLeft: number): string {
  const seconds = Math.max(0, Math.ceil(msLeft / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** An SVG path drawing `qr`'s dark modules, one rectangle per run of them in a row, in module units. */
export function qrPath(qr: Qr): string {
  let path = "";
  for (let y = 0; y < qr.size; y++) {
    let x = 0;
    while (x < qr.size) {
      if (!qr.modules[y * qr.size + x]) {
        x++;
        continue;
      }
      const start = x;
      while (x < qr.size && qr.modules[y * qr.size + x]) x++;
      const run = x - start;
      path += `M${start} ${y}h${run}v1h-${run}z`;
    }
  }
  return path;
}

export function isPairingLink(text: string): boolean {
  return text.trim().startsWith("clipsync://pair?");
}

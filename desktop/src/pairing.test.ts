import assert from "node:assert/strict";
import { test } from "node:test";

import { answered, countdown, isPairingLink, onPairingEvent, type PairState, qrPath, showingCode } from "./pairing.ts";
import type { Invite } from "./status.ts";

const invite: Invite = { uri: "clipsync://pair?v=1", expires_in_s: 120, qr: { size: 2, modules: [true, false, true, true] } };

test("a code shown now works until two minutes from now", () => {
  const state = showingCode(invite, 1_000);
  assert.deepEqual(state, { kind: "code", invite, expiresAt: 121_000, asking: null });
});

test("a device comparing codes is asked about, then the answer clears the question", () => {
  const asked = onPairingEvent(showingCode(invite, 0), { type: "code", name: "beta", code: "123 456" });
  assert.deepEqual(asked.kind === "code" && asked.asking, { name: "beta", code: "123 456" });
  const after = answered(asked);
  assert.equal(after.kind === "code" && after.asking, null);
});

test("pairing ends paired, expired or failed", () => {
  const code = showingCode(invite, 0);
  assert.deepEqual(onPairingEvent(code, { type: "paired", name: "phone" }), { kind: "paired", name: "phone" });
  assert.deepEqual(onPairingEvent(code, { type: "ended" }), { kind: "expired" });
  assert.deepEqual(onPairingEvent(code, { type: "failed", reason: "gone" }), { kind: "failed", reason: "gone", retry: "code" });
  const link: PairState = { kind: "link", busy: true };
  assert.deepEqual(onPairingEvent(link, { type: "failed", reason: "bad" }), { kind: "failed", reason: "bad", retry: "link" });
  assert.deepEqual(onPairingEvent(link, { type: "paired", name: "desk" }), { kind: "paired", name: "desk" });
});

test("events that do not apply leave the state as it is", () => {
  const link: PairState = { kind: "link", busy: true };
  assert.equal(onPairingEvent(link, { type: "code", name: "x", code: "1" }), link);
  assert.equal(onPairingEvent(link, { type: "ended" }), link);
  const paired: PairState = { kind: "paired", name: "phone" };
  assert.equal(onPairingEvent(paired, { type: "failed", reason: "late" }), paired);
});

test("the countdown shows minutes and seconds, rounded up, never below zero", () => {
  assert.equal(countdown(120_000), "2:00");
  assert.equal(countdown(119_001), "2:00");
  assert.equal(countdown(61_000), "1:01");
  assert.equal(countdown(9_000), "0:09");
  assert.equal(countdown(-5), "0:00");
});

test("the QR path draws each row's dark runs as rectangles", () => {
  assert.equal(qrPath({ size: 3, modules: [true, true, false, false, false, false, true, false, true] }), "M0 0h2v1h-2zM0 2h1v1h-1zM2 2h1v1h-1z");
  assert.equal(qrPath({ size: 2, modules: [false, false, false, false] }), "");
});

test("only a clipsync pairing link can be pasted", () => {
  assert.equal(isPairingLink("  clipsync://pair?v=1&id=ab  "), true);
  assert.equal(isPairingLink("https://example.com"), false);
  assert.equal(isPairingLink(""), false);
});

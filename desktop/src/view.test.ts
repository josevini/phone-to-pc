import assert from "node:assert/strict";
import { test } from "node:test";

import type { Device, Shown, Status } from "./status.ts";
import { devicePage, home, nameProblem } from "./view.ts";

function status(paused: boolean, devices: Device[]): Status {
  return { id: "86224755".padEnd(64, "0"), name: "book2", port: 47823, addrs: [], pairing: false, paused, devices };
}

function shown(status: Status | null, summary = "summary", sharing: boolean | null = true): Shown {
  return { status, view: { summary, sharing, dimmed: false } };
}

test("a running daemon shows this device, the switch and the paired devices", () => {
  const devices = [
    { id: "a".repeat(64), name: "phone", connected: true },
    { id: "b".repeat(64), name: "desktop", connected: false },
  ];
  const model = home(shown(status(false, devices), "Connected to 1 device"));
  assert.equal(model.summary, "Connected to 1 device");
  assert.deepEqual(model.device, { name: "book2", id: "ID 86224755" });
  assert.equal(model.sharing, true);
  assert.equal(model.sharingHint, "Sending to and receiving from paired devices");
  assert.deepEqual(model.devices, [
    { id: "a".repeat(64), name: "phone", state: "Connected", connected: true },
    { id: "b".repeat(64), name: "desktop", state: "Not connected", connected: false },
  ]);
  assert.equal(model.daemonDown, false);
  assert.equal(model.noDevices, false);
});

test("while paused the switch is off and says what pausing does", () => {
  const model = home(shown(status(true, []), "Sharing paused", false));
  assert.equal(model.sharing, false);
  assert.equal(model.sharingHint, "Paused: nothing is sent or received. Devices stay paired and connected.");
});

test("without paired devices the window says how to pair", () => {
  assert.equal(home(shown(status(false, []))).noDevices, true);
});

test("without a daemon there is no device and no switch", () => {
  const model = home(shown(null, "The clipsync daemon is not running", null));
  assert.equal(model.daemonDown, true);
  assert.equal(model.device, null);
  assert.equal(model.sharing, null);
  assert.deepEqual(model.devices, []);
  assert.equal(model.noDevices, false);
});

test("a device's page shows its full ID and whether it is connected", () => {
  const devices = [{ id: "a".repeat(64), name: "phone", connected: true }];
  assert.deepEqual(devicePage(status(false, devices), "a".repeat(64)), {
    name: "phone",
    id: "a".repeat(64),
    state: "Connected",
    connected: true,
  });
});

test("a device that is no longer paired has no page", () => {
  assert.equal(devicePage(status(false, []), "a".repeat(64)), null);
  assert.equal(devicePage(null, "a".repeat(64)), null);
});

test("a device name has 1 to 64 bytes once trimmed", () => {
  assert.equal(nameProblem("Meu PC"), null);
  assert.equal(nameProblem("  "), "Enter a name.");
  assert.equal(nameProblem("a".repeat(64)), null);
  assert.equal(nameProblem("a".repeat(65)), "Use at most 64 bytes.");
  // 22 three-byte characters are 66 bytes.
  assert.equal(nameProblem("日".repeat(22)), "Use at most 64 bytes.");
});

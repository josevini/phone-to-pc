import type { Shown } from "./status.ts";
import { type Home, home } from "./view.ts";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`#${id} is missing`);
  return found as T;
}

const summary = element("summary");
const down = element("down");
const thisDevice = element("this-device");
const deviceName = element("device-name");
const deviceId = element("device-id");
const sharing = element<HTMLInputElement>("sharing");
const sharingHint = element("sharing-hint");
const paired = element("paired");
const devices = element<HTMLUListElement>("devices");
const noDevices = element("no-devices");
const error = element("error");

let last: Home | null = null;

/** Draws `model`. Device names come from other devices: they are only ever set as text. */
function draw(model: Home): void {
  last = model;
  summary.textContent = model.summary;
  down.hidden = !model.daemonDown;
  thisDevice.hidden = model.daemonDown;
  paired.hidden = model.daemonDown;
  deviceName.textContent = model.device?.name ?? "";
  deviceId.textContent = model.device?.id ?? "";
  sharing.checked = model.sharing ?? false;
  sharing.disabled = model.sharing === null;
  sharingHint.textContent = model.sharingHint;
  devices.replaceChildren(
    ...model.devices.map((device) => {
      const row = document.createElement("li");
      row.className = "row";
      const name = document.createElement("div");
      name.className = "title";
      name.textContent = device.name;
      const state = document.createElement("div");
      state.className = device.connected ? "subtitle connected" : "subtitle";
      state.textContent = device.state;
      const text = document.createElement("div");
      text.append(name, state);
      row.append(text);
      return row;
    }),
  );
  devices.hidden = model.devices.length === 0;
  noDevices.hidden = !model.noDevices;
}

function show(shown: Shown): void {
  error.hidden = true;
  draw(home(shown));
}

sharing.addEventListener("change", () => {
  invoke("set_paused", { paused: !sharing.checked }).catch((e: unknown) => {
    // The daemon did not take it: show the state it still has.
    if (last) draw(last);
    error.textContent = `Could not change sharing: ${String(e)}`;
    error.hidden = false;
  });
});

await listen<Shown>("shown", (event) => show(event.payload));
show(await invoke<Shown>("shown"));

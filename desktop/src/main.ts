import { answered, countdown, isPairingLink, onPairingEvent, type PairState, qrPath, showingCode } from "./pairing.ts";
import type { Invite, PairingEvent, Shown } from "./status.ts";
import { type Home, home } from "./view.ts";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`#${id} is missing`);
  return found as T;
}

// ---------------------------------------------------------------- home

const homeView = element("home");
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
function drawHome(model: Home): void {
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

function showStatus(shown: Shown): void {
  error.hidden = true;
  drawHome(home(shown));
}

sharing.addEventListener("change", () => {
  invoke("set_paused", { paused: !sharing.checked }).catch((e: unknown) => {
    // The daemon did not take it: show the state it still has.
    if (last) drawHome(last);
    error.textContent = `Could not change sharing: ${String(e)}`;
    error.hidden = false;
  });
});

// ---------------------------------------------------------------- pairing

const pairView = element("pair");
const pairCancel = element("pair-cancel");
const pairTitle = element("pair-title");
const pairCode = element("pair-code");
const qr = element("qr") as unknown as SVGSVGElement;
const countdownText = element("countdown");
const pairUri = element("pair-uri");
const confirm = element("pair-confirm");
const confirmTitle = element("confirm-title");
const confirmCode = element("confirm-code");
const linkForm = element<HTMLFormElement>("pair-link");
const link = element<HTMLInputElement>("link");
const linkSubmit = element<HTMLButtonElement>("link-submit");
const result = element("pair-result");
const resultTitle = element("result-title");
const resultText = element("result-text");
const resultAction = element<HTMLButtonElement>("result-action");

/** The pairing on screen; null on the home screen. */
let pair: PairState | null = null;
let ticking: number | undefined;

function setPair(state: PairState | null): void {
  const before = pair;
  pair = state;
  drawPair();
  // Keyboard focus follows the screen: to what it asks for, or to its way out.
  if (state?.kind !== before?.kind || (state?.kind === "code" && before?.kind === "code" && state.asking !== before.asking)) {
    if (state === null) element("show-code").focus();
    else if (state.kind === "code") (state.asking ? element("confirm-no") : pairCancel).focus();
    else if (state.kind === "link") link.focus();
    else resultAction.focus();
  }
}

function drawPair(): void {
  homeView.hidden = pair !== null;
  pairView.hidden = pair === null;
  window.clearInterval(ticking);
  ticking = undefined;
  if (pair === null) return;

  pairCancel.hidden = pair.kind === "paired";
  pairCode.hidden = pair.kind !== "code" || pair.asking !== null;
  confirm.hidden = pair.kind !== "code" || pair.asking === null;
  linkForm.hidden = pair.kind !== "link";
  result.hidden = pair.kind === "code" || pair.kind === "link";

  switch (pair.kind) {
    case "code": {
      pairTitle.textContent = "Show a pairing code";
      const { invite, expiresAt, asking } = pair;
      qr.setAttribute("viewBox", `-4 -4 ${invite.qr.size + 8} ${invite.qr.size + 8}`);
      qr.querySelector("path")?.setAttribute("d", qrPath(invite.qr));
      pairUri.textContent = invite.uri;
      const tick = () => {
        countdownText.textContent = `The code works for ${countdown(expiresAt - Date.now())} more`;
      };
      tick();
      ticking = window.setInterval(tick, 1000);
      if (asking) {
        confirmTitle.textContent = `${asking.name} wants to pair`;
        confirmCode.textContent = asking.code;
      }
      break;
    }
    case "link":
      pairTitle.textContent = "Pair with a link";
      link.disabled = pair.busy;
      linkSubmit.disabled = pair.busy || !isPairingLink(link.value);
      linkSubmit.textContent = pair.busy ? "Pairing…" : "Pair";
      break;
    case "paired":
      pairTitle.textContent = "Paired";
      resultTitle.textContent = `Paired with ${pair.name}`;
      resultText.textContent = "The two devices now share the clipboard while they are connected.";
      resultAction.textContent = "Done";
      break;
    case "expired":
      pairTitle.textContent = "Show a pairing code";
      resultTitle.textContent = "The code expired";
      resultText.textContent = "A code works for one pairing, for a short time. Show a new code to try again.";
      resultAction.textContent = "Show a new code";
      break;
    case "failed":
      pairTitle.textContent = "Pairing failed";
      resultTitle.textContent = "Pairing failed";
      resultText.textContent = pair.reason;
      resultAction.textContent = "Try again";
      break;
  }
}

async function showCode(): Promise<void> {
  try {
    const invite = await invoke<Invite>("show_code");
    setPair(showingCode(invite, Date.now()));
  } catch (e) {
    setPair({ kind: "failed", reason: String(e), retry: "code" });
  }
}

function enterLink(): void {
  link.value = "";
  setPair({ kind: "link", busy: false });
}

/** Back to the home screen; a code shown here stops working. */
function cancel(): void {
  if (pair === null) return;
  invoke("stop_pairing").catch(() => {});
  setPair(null);
}

element("show-code").addEventListener("click", () => void showCode());
element("enter-link").addEventListener("click", enterLink);
element("pair-cancel").addEventListener("click", cancel);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") cancel();
});

link.addEventListener("input", drawPair);
linkForm.addEventListener("submit", (event) => {
  event.preventDefault();
  if (!isPairingLink(link.value)) return;
  setPair({ kind: "link", busy: true });
  invoke("pair_with_link", { uri: link.value }).catch((e: unknown) => {
    setPair({ kind: "failed", reason: String(e), retry: "link" });
  });
});

for (const [id, accept] of [["confirm-yes", true], ["confirm-no", false]] as const) {
  element(id).addEventListener("click", () => {
    invoke("confirm_pairing", { accept }).catch(() => {});
    if (pair) setPair(answered(pair));
  });
}

resultAction.addEventListener("click", () => {
  if (pair?.kind === "expired" || (pair?.kind === "failed" && pair.retry === "code")) void showCode();
  else if (pair?.kind === "failed") enterLink();
  else setPair(null);
});

await listen<PairingEvent>("pairing", (event) => {
  if (pair) setPair(onPairingEvent(pair, event.payload));
});
// Hiding the window stops the pairing: start over on the home screen.
await listen("closed", () => setPair(null));
await listen<Shown>("shown", (event) => showStatus(event.payload));
showStatus(await invoke<Shown>("shown"));

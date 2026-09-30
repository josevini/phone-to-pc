import type { Shown, Status } from "./status.ts";

/** Everything the window shows, worked out from what the app reports; the DOM code only draws it. */
export interface Home {
  summary: string;
  /** This device, while the daemon runs. */
  device: { name: string; id: string } | null;
  /** "Share the clipboard": null while there is no daemon to ask. */
  sharing: boolean | null;
  sharingHint: string;
  devices: { id: string; name: string; state: string; connected: boolean }[];
  /** No paired devices yet: say how to pair. */
  noDevices: boolean;
  daemonDown: boolean;
}

export function home({ status, view }: Shown): Home {
  const sharing = status ? view.sharing : null;
  return {
    summary: view.summary,
    device: status ? { name: status.name, id: `ID ${status.id.slice(0, 8)}` } : null,
    sharing,
    sharingHint:
      sharing === false
        ? "Paused: nothing is sent or received. Devices stay paired and connected."
        : "Sending to and receiving from paired devices",
    devices: (status?.devices ?? []).map((d) => ({
      id: d.id,
      name: d.name,
      state: d.connected ? "Connected" : "Not connected",
      connected: d.connected,
    })),
    noDevices: status !== null && status.devices.length === 0,
    daemonDown: status === null,
  };
}

/** A paired device's page. */
export interface DevicePage {
  name: string;
  id: string;
  state: string;
  connected: boolean;
}

/** The page of device `id`, or null once it is no longer paired (unpaired here or from the other device). */
export function devicePage(status: Status | null, id: string): DevicePage | null {
  const device = status?.devices.find((d) => d.id === id);
  if (!device) return null;
  return { name: device.name, id: device.id, state: device.connected ? "Connected" : "Not connected", connected: device.connected };
}

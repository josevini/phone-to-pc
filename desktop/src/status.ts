/** The daemon's status, as its control socket reports it (`StatusView` in clipsyncd's `ipc.rs`). */
export interface Status {
  id: string;
  name: string;
  port: number;
  addrs: string[];
  pairing: boolean;
  paused: boolean;
  devices: Device[];
}

export interface Device {
  id: string;
  name: string;
  connected: boolean;
}

/** The tray's view of the status, worked out by the app (`TrayView` in `src-tauri/src/tray.rs`). */
export interface TrayView {
  summary: string;
  /** Null while there is no daemon to ask. */
  sharing: boolean | null;
  dimmed: boolean;
}

/** What the window shows: the `shown` command's result and the `shown` event's payload. */
export interface Shown {
  status: Status | null;
  view: TrayView;
}

/** A QR code's modules, row by row; true is dark (`Qr` in `src-tauri/src/qr.rs`). */
export interface Qr {
  size: number;
  modules: boolean[];
}

/** This device's pairing code (`Invite` in `src-tauri/src/pairing.rs`). */
export interface Invite {
  uri: string;
  expires_in_s: number;
  qr: Qr;
}

/** What happens during a pairing (`PairingEvent` in `src-tauri/src/pairing.rs`). */
export type PairingEvent =
  | { type: "code"; name: string; code: string }
  | { type: "paired"; name: string }
  | { type: "failed"; reason: string }
  | { type: "ended" };

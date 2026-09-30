/** The part of Tauri's global API (`app.withGlobalTauri`) the window uses. */
interface Window {
  __TAURI__: {
    core: {
      invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
    };
    event: {
      listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<() => void>;
    };
  };
}

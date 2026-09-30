//! The connection to the clipsync daemon, through its control socket.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, bail};
use clipsyncd::ipc::{Client, Reply, Request, StatusView};

/// Follows the status of the daemon listening at `socket`, forever: calls `on_status` with the status each time it
/// changes, and with `None` once whenever the daemon cannot be reached. Tries again every `retry` meanwhile.
pub async fn watch(socket: PathBuf, retry: Duration, mut on_status: impl FnMut(Option<StatusView>)) {
    let mut reachable = None;
    loop {
        if let Ok(mut client) = Client::connect(&socket).await
            && client.send(&Request::Subscribe).await.is_ok()
        {
            while let Ok(Some(reply)) = client.next().await {
                if let Reply::Status { status } = reply {
                    reachable = Some(true);
                    on_status(Some(status));
                }
            }
        }
        if reachable != Some(false) {
            reachable = Some(false);
            on_status(None);
        }
        tokio::time::sleep(retry).await;
    }
}

/// Sends one request to the daemon at `socket` and returns its reply.
pub async fn request(socket: &Path, request: &Request) -> Result<Reply> {
    Client::connect(socket).await?.request(request).await
}

/// Unpairs device `id` (its full ID) through the daemon at `socket`, which tells the device if it is connected.
/// Returns the device's name.
pub async fn unpair(socket: &Path, id: &str) -> Result<String> {
    match request(socket, &Request::Unpair { device: id.into() }).await? {
        Reply::Unpaired { name, .. } => Ok(name),
        Reply::Error { message } => bail!(message),
        other => bail!("unexpected reply from the daemon: {other:?}"),
    }
}

/// Renames this device through the daemon at `socket`.
pub async fn rename(socket: &Path, name: &str) -> Result<()> {
    match request(socket, &Request::Rename { name: name.into() }).await? {
        Reply::Ok => Ok(()),
        Reply::Error { message } => bail!(message),
        other => bail!("unexpected reply from the daemon: {other:?}"),
    }
}

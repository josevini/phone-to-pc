//! Sockets: accepting and dialing TLS connections, and pumping bytes to and from the actor.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clipsync_core::DeviceId;
use clipsync_core::engine::{ConnId, Intent, Role};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use super::Cmd;
use crate::tls::Tls;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Allocates connection IDs shared by accepted and dialed connections.
#[derive(Clone, Default)]
pub(super) struct ConnIds(Arc<AtomicU64>);

impl ConnIds {
    fn next(&self) -> ConnId {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Accepts connections until the actor goes away.
pub(super) async fn accept_loop(listener: TcpListener, tls: Tls, ids: ConnIds, cmds: mpsc::UnboundedSender<Cmd>) {
    loop {
        let (tcp, addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                warn!("accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        if cmds.is_closed() {
            return;
        }
        let (tls, ids, cmds) = (tls.clone(), ids.clone(), cmds.clone());
        tokio::spawn(async move {
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, tls.accept(tcp)).await {
                Ok(Ok((stream, peer))) => {
                    run_connection(stream, ids.next(), Role::Acceptor, peer, Intent::Session, None, cmds).await
                }
                Ok(Err(e)) => debug!(%addr, "TLS handshake refused: {e:#}"),
                Err(_) => debug!(%addr, "TLS handshake timed out"),
            }
        });
    }
}

/// Dials `addrs` in order until one works; reports the outcome to the actor.
pub(super) fn dial(
    addrs: Vec<SocketAddr>,
    expected: Option<DeviceId>,
    intent: Intent,
    tls: Tls,
    ids: ConnIds,
    cmds: mpsc::UnboundedSender<Cmd>,
) {
    tokio::spawn(async move {
        let mut last_error = anyhow::anyhow!("no address to dial");
        for addr in &addrs {
            match connect(*addr, expected, &tls).await {
                Ok((stream, peer)) => {
                    let origin = Some(*addr);
                    return run_connection(stream, ids.next(), Role::Dialer, peer, intent, origin, cmds).await;
                }
                Err(e) => last_error = e,
            }
        }
        let _ = cmds.send(Cmd::DialFailed { addrs, error: format!("{last_error:#}") });
    });
}

async fn connect(
    addr: SocketAddr,
    expected: Option<DeviceId>,
    tls: &Tls,
) -> Result<(tokio_rustls::client::TlsStream<TcpStream>, DeviceId)> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .with_context(|| format!("connecting to {addr} timed out"))?
        .with_context(|| format!("connecting to {addr}"))?;
    tcp.set_nodelay(true)?;
    let (stream, peer) = tokio::time::timeout(HANDSHAKE_TIMEOUT, tls.connect(tcp))
        .await
        .with_context(|| format!("TLS handshake with {addr} timed out"))??;
    if let Some(expected) = expected
        && expected != peer
    {
        bail!("{addr} is device {}, not the expected {}", peer.short(), expected.short());
    }
    Ok((stream, peer))
}

/// Hands the connection to the actor and moves bytes until either side closes it.
async fn run_connection<S>(
    stream: S,
    conn: ConnId,
    role: Role,
    peer: DeviceId,
    intent: Intent,
    dialed: Option<SocketAddr>,
    cmds: mpsc::UnboundedSender<Cmd>,
) where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (writes, mut pending) = mpsc::unbounded_channel::<Vec<u8>>();
    let (stop_reading, mut stopped) = oneshot::channel::<()>();
    if cmds.send(Cmd::Opened { conn, role, peer, intent, dialed, writer: writes }).is_err() {
        return;
    }

    // The actor closes the connection by dropping `writes`.
    tokio::spawn(async move {
        while let Some(bytes) = pending.recv().await {
            if writer.write_all(&bytes).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
        let _ = stop_reading.send(());
    });

    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read = reader.read(&mut buf) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if cmds.send(Cmd::Bytes { conn, bytes: buf[..n].to_vec() }).is_err() {
                        return;
                    }
                }
            },
            _ = &mut stopped => break,
        }
    }
    let _ = cmds.send(Cmd::Closed { conn });
}

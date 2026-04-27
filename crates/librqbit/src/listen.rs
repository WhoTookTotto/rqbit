use std::{
    net::{Ipv6Addr, SocketAddr},
    sync::Arc,
};

use anyhow::Context;
use librqbit_dualstack_sockets::{BindOpts, TcpListener};
use librqbit_dualstack_sockets::UdpSocket as DsUdpSocket;
use librqbit_utp::{BindDevice, UtpSocket, UtpSocketUdp, UtpSocketUdpOpts};
use tokio::io::AsyncWrite;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{stream_connect::ConnectionKind, vectored_traits::AsyncReadVectored};

pub(crate) struct ListenResult {
    pub tcp_socket: Option<TcpListener>,
    pub utp_socket: Option<Arc<UtpSocketUdp>>,
    pub enable_upnp_port_forwarding: bool,
    pub addr: SocketAddr,
    pub announce_port: Option<u16>,
}

#[derive(Debug, Clone, Copy)]
pub enum ListenerMode {
    TcpOnly,
    UtpOnly,
    TcpAndUtp,
}

impl ListenerMode {
    pub fn tcp_enabled(&self) -> bool {
        match self {
            ListenerMode::TcpOnly => true,
            ListenerMode::UtpOnly => false,
            ListenerMode::TcpAndUtp => true,
        }
    }

    pub fn utp_enabled(&self) -> bool {
        match self {
            ListenerMode::TcpOnly => false,
            ListenerMode::UtpOnly => true,
            ListenerMode::TcpAndUtp => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListenerOptions {
    pub mode: ListenerMode,
    pub listen_addr: SocketAddr,
    pub enable_upnp_port_forwarding: bool,
    pub utp_opts: Option<librqbit_utp::SocketOpts>,
    pub announce_port: Option<u16>,
    pub ipv4_only: bool,
    /// If set, TCP and uTP sockets are created inside this network namespace.
    #[cfg(target_os = "linux")]
    pub netns: Option<Arc<crate::netns_util::NetNs>>,
}

impl Default for ListenerOptions {
    fn default() -> Self {
        Self {
            // TODO: once uTP is stable upgrade default to both
            mode: ListenerMode::TcpOnly,
            listen_addr: (Ipv6Addr::UNSPECIFIED, 0).into(),
            enable_upnp_port_forwarding: false,
            utp_opts: None,
            announce_port: None,
            ipv4_only: false,
            #[cfg(target_os = "linux")]
            netns: None,
        }
    }
}

impl ListenerOptions {
    pub(crate) async fn start(
        mut self,
        parent_span: Option<tracing::Id>,
        cancellation_token: CancellationToken,
        bind_device: Option<&BindDevice>,
    ) -> anyhow::Result<ListenResult> {
        #[cfg(target_os = "linux")]
        let netns = self.netns.take();

        let mut utp_opts = self.utp_opts.take().unwrap_or_default();
        utp_opts.cancellation_token = cancellation_token.clone();
        utp_opts.parent_span = parent_span;
        utp_opts.dont_wait_for_lastack = true;

        let mut listen_addr = if self.ipv4_only {
            if self.listen_addr.is_ipv6() && self.listen_addr.ip().is_unspecified() {
                // Force to IPv4 unspecified if IPv6 unspecified was requested but we are v4 only
                SocketAddr::from(([0, 0, 0, 0], self.listen_addr.port()))
            } else {
                self.listen_addr
            }
        } else {
            self.listen_addr
        };

        let tcp_socket = if self.mode.tcp_enabled() {
            let listener = {
                let dualstack = !self.ipv4_only;
                #[cfg(target_os = "linux")]
                {
                    if netns.is_some() {
                        let ns_ref = netns.as_ref();
                        let listen_addr_c = listen_addr;
                        crate::netns_util::run_in_netns(ns_ref, move || {
                            TcpListener::bind_tcp(
                                listen_addr_c,
                                BindOpts { request_dualstack: dualstack, reuseport: false, device: None },
                            )
                            .context("error starting TCP listener")
                        })
                        .await?
                    } else {
                        TcpListener::bind_tcp(
                            listen_addr,
                            BindOpts {
                                request_dualstack: dualstack,
                                reuseport: false,
                                device: bind_device,
                            },
                        )
                        .context("error starting TCP listener")?
                    }
                }
                #[cfg(not(target_os = "linux"))]
                TcpListener::bind_tcp(
                    listen_addr,
                    BindOpts { request_dualstack: dualstack, reuseport: false, device: bind_device },
                )
                .context("error starting TCP listener")?
            };
            listen_addr = listener.bind_addr();
            info!(
                "Listening on TCP {:?} for incoming peer connections",
                listen_addr
            );
            Some(listener)
        } else {
            None
        };

        let utp_socket = if self.mode.utp_enabled() {
            let bind_result: Result<Arc<UtpSocketUdp>, anyhow::Error> = {
                #[cfg(target_os = "linux")]
                {
                    if let Some(ref ns) = netns {
                        // Pre-create the raw UDP socket in the namespace (synchronous bind_udp),
                        // then hand it to UtpSocket::new_with_opts (also synchronous).
                        let ns = ns.clone();
                        let listen_addr_c = listen_addr;
                        let udp_sock = tokio::task::spawn_blocking(move || {
                            ns.run(|_| {
                                DsUdpSocket::bind_udp(
                                    listen_addr_c,
                                    BindOpts { request_dualstack: true, reuseport: false, device: None },
                                )
                                .map_err(|e| anyhow::anyhow!("uTP udp bind failed: {e}"))
                            })
                            .map_err(|e| anyhow::anyhow!("netns enter failed: {e}"))?
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))??;
                        UtpSocket::new_with_opts(udp_sock, Default::default(), utp_opts)
                            .map_err(anyhow::Error::from)
                    } else {
                        UtpSocketUdp::new_udp_with_opts(
                            listen_addr,
                            utp_opts,
                            UtpSocketUdpOpts { bind_device },
                        )
                        .await
                        .map_err(anyhow::Error::from)
                    }
                }
                #[cfg(not(target_os = "linux"))]
                UtpSocketUdp::new_udp_with_opts(
                    listen_addr,
                    utp_opts,
                    UtpSocketUdpOpts { bind_device },
                )
                .await
                .map_err(anyhow::Error::from)
            };
            match bind_result {
                Ok(sock) => {
                    listen_addr = sock.bind_addr();
                    info!(
                        "Listening on UDP {:?} for incoming uTP peer connections",
                        listen_addr
                    );
                    Some(sock)
                }
                Err(e) if tcp_socket.is_some() => {
                    // If we listen over TCP, it's not a fatal error if we can't listen over uTP.
                    tracing::error!("Error listening on UDP {listen_addr:?}: {e:#}");
                    None
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        } else {
            None
        };

        let announce_port = if let Some(p) = self.announce_port {
            Some(p)
        } else if listen_addr.ip().is_loopback() {
            None
        } else {
            Some(listen_addr.port())
        };
        Ok(ListenResult {
            tcp_socket,
            utp_socket,
            announce_port,
            addr: listen_addr,
            enable_upnp_port_forwarding: self.enable_upnp_port_forwarding,
        })
    }
}

pub(crate) trait Accept {
    const KIND: ConnectionKind;

    async fn accept(
        &self,
    ) -> anyhow::Result<(
        SocketAddr,
        (
            impl AsyncReadVectored + Send + 'static,
            impl AsyncWrite + Unpin + Send + 'static,
        ),
    )>;
}

impl Accept for TcpListener {
    const KIND: ConnectionKind = ConnectionKind::Tcp;
    async fn accept(
        &self,
    ) -> anyhow::Result<(
        SocketAddr,
        (
            impl AsyncReadVectored + Send + 'static,
            impl AsyncWrite + Send + 'static,
        ),
    )> {
        let (stream, addr) = self.accept().await.context("error accepting TCP")?;
        let (read, write) = stream.into_split();
        Ok((addr, (read, write)))
    }
}

impl Accept for Arc<UtpSocketUdp> {
    const KIND: ConnectionKind = ConnectionKind::Utp;
    async fn accept(
        &self,
    ) -> anyhow::Result<(
        SocketAddr,
        (
            impl AsyncReadVectored + Send + 'static,
            impl AsyncWrite + Unpin + Send + 'static,
        ),
    )> {
        let stream = self.accept().await.context("error accepting uTP")?;
        let addr = stream.remote_addr();
        let (read, write) = stream.split();
        Ok((addr, (read, write)))
    }
}

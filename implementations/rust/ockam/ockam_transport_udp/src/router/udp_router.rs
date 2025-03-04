use std::ops::Deref;
use std::{collections::BTreeMap, str::FromStr};

use futures_util::StreamExt;
use ockam_core::{async_trait, Address, Any, Decodable, LocalMessage, Result, Routed, Worker};
use ockam_node::Context;

use ockam_transport_core::TransportError;
use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;
use tracing::{error, trace};

use crate::router::{UdpRouterHandle, UdpRouterMessage};
use crate::transport::UdpAddress;
use crate::workers::{TransportMessageCodec, UdpListenProcessor, UdpSendWorker};

/// A UDP address router and listener
///
/// In order to create new UDP workers you need a router
/// to map remote addresses of `type = 2` to worker addresses.
/// This type facilitates this.
///
/// Optionally you can also start listening for incoming datagrams
/// if the local node is part of a server architecture.
pub(crate) struct UdpRouter {
    ctx: Context,
    main_addr: Address,
    api_addr: Address,
    map: BTreeMap<Address, Address>,
    allow_auto_connection: bool,
}

impl UdpRouter {
    /// Create and register a new UDP router with the node context
    pub(crate) async fn register(ctx: &Context) -> Result<UdpRouterHandle> {
        let main_addr = Address::random_local();
        let api_addr = Address::random_local();

        let child_ctx = ctx.new_detached(Address::random_local()).await?;

        let router = Self {
            ctx: child_ctx,
            main_addr: main_addr.clone(),
            api_addr: api_addr.clone(),
            map: BTreeMap::new(),
            allow_auto_connection: true,
        };

        let handle = router.create_self_handle(ctx).await?;

        ctx.start_worker(vec![main_addr.clone(), api_addr], router)
            .await?;
        trace!("Registering UDP router for type = {}", crate::UDP);
        ctx.register(crate::UDP, main_addr).await?;

        Ok(handle)
    }

    /// Create a new `UdpRouterHandle` representing this router
    async fn create_self_handle(&self, ctx: &Context) -> Result<UdpRouterHandle> {
        let handle_ctx = ctx.new_detached(Address::random_local()).await?;
        let handle = UdpRouterHandle::new(handle_ctx, self.api_addr.clone());
        Ok(handle)
    }

    async fn handle_route(&mut self, ctx: &Context, mut msg: LocalMessage) -> Result<()> {
        trace!(
            "UDP route request: {:?}",
            msg.transport().onward_route.next()
        );

        let onward = msg.transport().onward_route.next()?.clone();

        let next = if let Some(n) = self.map.get(&onward) {
            n.clone()
        } else {
            let peer_str = match String::from_utf8(onward.deref().clone()) {
                Ok(s) => s,
                Err(_e) => return Err(TransportError::UnknownRoute.into()),
            };

            if self.allow_auto_connection {
                self.connect(peer_str).await?
            } else {
                return Err(TransportError::UnknownRoute.into());
            }
        };

        let transport_msg = msg.transport_mut();
        transport_msg.onward_route.step()?;
        // Prepend peer socket addr so that sender can use it
        transport_msg.onward_route.modify().prepend(onward);
        transport_msg.onward_route.modify().prepend(next.clone());

        ctx.send(next.clone(), msg).await?;

        Ok(())
    }

    async fn handle_register(&mut self, accepts: Vec<Address>, self_addr: Address) -> Result<()> {
        if let Some(f) = accepts.first().cloned() {
            trace!("UDP registration request: {} => {}", f, self_addr);
        } else {
            error!("Tried to register a new client without passing any `Address`");
            return Err(TransportError::InvalidAddress.into());
        }

        for accept in &accepts {
            if self.map.contains_key(accept) {
                // TODO: is returning OK right if addr(s) are already registered
                return Ok(());
            }
        }

        for accept in accepts {
            self.map.insert(accept, self_addr.clone());
        }

        Ok(())
    }

    async fn connect(&mut self, peer: String) -> Result<Address> {
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .map_err(TransportError::from)?;
        let (sink, stream) = UdpFramed::new(socket, TransportMessageCodec).split();

        let tx_addr = Address::random_local();
        let sender = UdpSendWorker::new(sink);
        self.ctx.start_worker(tx_addr.clone(), sender).await?;
        UdpListenProcessor::start(
            &self.ctx,
            stream,
            tx_addr.clone(),
            self.create_self_handle(&self.ctx).await?,
        )
        .await?;

        let (peer, hostnames) = UdpRouterHandle::resolve_peer(peer)?;
        let mut accepts: Vec<Address> = vec![UdpAddress::from(peer).into()];
        accepts.extend(
            hostnames
                .iter()
                .filter_map(|s| UdpAddress::from_str(s).ok())
                .map(|addr| addr.into()),
        );

        self.handle_register(accepts, tx_addr.clone()).await?;

        Ok(tx_addr)
    }
}

#[async_trait]
impl Worker for UdpRouter {
    type Message = Any;
    type Context = Context;

    async fn initialize(&mut self, ctx: &mut Context) -> Result<()> {
        ctx.set_cluster(crate::CLUSTER_NAME).await?;
        Ok(())
    }

    async fn handle_message(&mut self, ctx: &mut Context, msg: Routed<Any>) -> Result<()> {
        let msg_addr = msg.msg_addr();

        if msg_addr == self.main_addr {
            self.handle_route(ctx, msg.into_local_message()).await?;
        } else if msg_addr == self.api_addr {
            let msg = UdpRouterMessage::decode(msg.payload())?;
            match msg {
                UdpRouterMessage::Register { accepts, self_addr } => {
                    trace!("handle_message register: {:?} => {:?}", accepts, self_addr);
                    self.handle_register(accepts, self_addr).await?;
                }
            };
        } else {
            return Err(TransportError::InvalidAddress.into());
        }

        Ok(())
    }
}

use std::{marker::PhantomData, sync::Arc};

use anyhow::{anyhow, bail};
use discv5::{enr::NodeId, Enr, TalkRequest};
use ethportal_api::{
    types::{
        accept_code::AcceptCodeList,
        distance::{Distance, Metric},
        network::Subnetwork,
        ping_extensions::{
            decode::PingExtension,
            extension_types::PingExtensionType,
            extensions::{
                type_0::ClientInfoRadiusCapabilities,
                type_1::BasicRadius,
                type_2::HistoryRadius,
                type_65535::{ErrorCodes, PingError},
            },
        },
        portal_wire::{
            Accept, Content, CustomPayload, FindContent, FindNodes, Message, Nodes, Offer, Ping,
            Pong,
        },
        protocol_versions::ProtocolVersion,
    },
    OverlayContentKey,
};
use portalnet::{discovery::UtpPeer, utp::controller::UTP_CONN_CFG};
use ssz::{Decode, Encode};
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, error, info, warn};
use utp_rs::{peer::Peer, socket::UtpSocket};

use crate::{discovery::Discovery, types::FindContentResult};

const FIND_NODES_MAX_DISTANCE_DIFFERENCE: u16 = 3;

#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    pub incoming_talk_request_capacity: usize,
    pub outgoing_talk_request_capacity: usize,
}

pub struct Protocol<TContentKey, TMetric> {
    config: ProtocolConfig,
    subnetwork: Subnetwork,
    discovery: Arc<Discovery>,
    utp_socket: Arc<UtpSocket<UtpPeer>>,
    outgoing_semaphore: Arc<Semaphore>,
    _phantom_content_key: PhantomData<TContentKey>,
    _phantom_metric: PhantomData<TMetric>,
}

impl<TContentKey, TMetric> Protocol<TContentKey, TMetric>
where
    TContentKey: 'static + Send + Sync + OverlayContentKey,
    TMetric: 'static + Send + Sync + Metric,
{
    pub fn spawn(
        config: ProtocolConfig,
        subnetwork: Subnetwork,
        discovery: Arc<Discovery>,
        utp_socket: Arc<UtpSocket<UtpPeer>>,
    ) -> anyhow::Result<Arc<Self>> {
        let protocol = Arc::new(Self::new(config, subnetwork, discovery, utp_socket));
        protocol.start();
        Ok(protocol)
    }

    pub fn new(
        config: ProtocolConfig,
        subnetwork: Subnetwork,
        discovery: Arc<Discovery>,
        utp_socket: Arc<UtpSocket<UtpPeer>>,
    ) -> Self {
        let outgoing_semaphore = Arc::new(Semaphore::new(config.outgoing_talk_request_capacity));
        Self {
            config,
            subnetwork,
            discovery,
            utp_socket,
            outgoing_semaphore,
            _phantom_content_key: PhantomData,
            _phantom_metric: PhantomData,
        }
    }

    pub fn start(self: &Arc<Self>) {
        let protocol = Arc::clone(self);

        let (query_rx, mut query_tx) =
            mpsc::channel(protocol.config.incoming_talk_request_capacity);
        protocol
            .discovery
            .register_handler(protocol.subnetwork, query_rx);

        tokio::spawn(async move {
            loop {
                let Some((enr, talk_request)) = query_tx.recv().await else {
                    warn!(subnetwork=%protocol.subnetwork, "Query channel closed");
                    break;
                };
                if let Err(err) = protocol.process_incoming_talk_request(enr, talk_request) {
                    error!(%err, "Error processing incoming TALKREQ");
                }
            }
        });
        info!(subnetwork=%self.subnetwork, "Subnetwork Service started");
    }

    fn process_incoming_talk_request(
        &self,
        enr: Enr,
        talk_request: TalkRequest,
    ) -> anyhow::Result<()> {
        let message = Message::from_ssz_bytes(talk_request.body()).map_err(|err| {
            anyhow!(
                "Error decoding incoming message! subnetwork={} err={err:?}",
                self.subnetwork
            )
        })?;

        debug!("Processing talk_request: {message:?}");

        let response = match message {
            Message::Ping(ping) => Message::Pong(self.process_incoming_ping(enr, ping)?),
            Message::FindNodes(find_nodes) => {
                Message::Nodes(self.process_incoming_find_nodes(enr, find_nodes)?)
            }
            Message::FindContent(find_content) => {
                Message::Content(self.process_incoming_find_content(enr, find_content)?)
            }
            Message::Offer(offer) => Message::Accept(self.process_incoming_offer(enr, offer)?),
            _ => {
                bail!(
                    "Unsupported TALKREQ! subnetwork={}, message={message:?}",
                    self.subnetwork
                );
            }
        };
        talk_request.respond(response.as_ssz_bytes())?;

        Ok(())
    }

    fn process_incoming_ping(&self, _enr: Enr, ping: Ping) -> anyhow::Result<Pong> {
        let local_enr = self.discovery.local_enr();

        let response_payload: CustomPayload =
            match PingExtension::decode_ssz(ping.payload_type, ping.payload) {
                Ok(PingExtension::Capabilities(_)) => ClientInfoRadiusCapabilities::new(
                    Distance::ZERO,
                    vec![
                        PingExtensionType::Capabilities,
                        PingExtensionType::HistoryRadius,
                        PingExtensionType::Error,
                    ],
                )
                .into(),
                Ok(PingExtension::BasicRadius(_)) => BasicRadius::new(Distance::ZERO).into(),
                Ok(PingExtension::HistoryRadius(_)) => HistoryRadius::new(Distance::ZERO, 0).into(),
                Ok(_) => PingError::new(ErrorCodes::ExtensionNotSupported).into(),
                Err(_err) => PingError::new(ErrorCodes::FailedToDecodePayload).into(),
            };
        Ok(Pong {
            enr_seq: local_enr.seq(),
            payload_type: ping.payload_type,
            payload: response_payload,
        })
    }

    fn process_incoming_find_nodes(
        &self,
        _enr: Enr,
        _find_nodes: FindNodes,
    ) -> anyhow::Result<Nodes> {
        Ok(Nodes {
            total: 1,
            enrs: vec![],
        })
    }

    fn process_incoming_find_content(
        &self,
        _enr: Enr,
        _find_content: FindContent,
    ) -> anyhow::Result<Content> {
        Ok(Content::Enrs(vec![]))
    }

    fn process_incoming_offer(&self, _enr: Enr, offer: Offer) -> anyhow::Result<Accept> {
        let accept_code_list = AcceptCodeList::new(offer.content_keys.len())
            .expect("should be able to create AcceptCodeList");
        Ok(Accept {
            connection_id: 0,
            content_keys: accept_code_list.encode(ProtocolVersion::V0)?,
        })
    }

    async fn send_talk_req(&self, enr: &Enr, message: Message) -> anyhow::Result<Message> {
        let response = self
            .discovery
            .send_talk_req(enr.clone(), self.subnetwork, message.as_ssz_bytes())
            .await
            .map_err(|err| {
                anyhow!(
                    "Error sending TALKREQ ({}): {err}",
                    message.as_ssz_bytes()[0],
                )
            })?;
        Message::from_ssz_bytes(&response)
            .map_err(|err| anyhow!("Error decoding TALKRESP: {err:?}"))
    }

    pub async fn send_ping(&self, enr: &Enr) -> anyhow::Result<Pong> {
        let _permit = self.outgoing_semaphore.acquire().await?;
        let ping = Ping {
            enr_seq: self.discovery.local_enr().seq(),
            payload_type: PingExtensionType::Capabilities,
            payload: ClientInfoRadiusCapabilities::new(
                Distance::ZERO,
                vec![
                    PingExtensionType::Capabilities,
                    PingExtensionType::HistoryRadius,
                    PingExtensionType::Error,
                ],
            )
            .into(),
        };
        match self.send_talk_req(enr, Message::Ping(ping)).await? {
            Message::Pong(pong) => Ok(pong),
            message => Err(anyhow!(
                "Wrong response type! Expected PONG received: {message:?}"
            )),
        }
    }

    pub async fn send_find_nodes(&self, enr: &Enr, target: &NodeId) -> anyhow::Result<Vec<Enr>> {
        let _permit = self.outgoing_semaphore.acquire().await?;
        let start_distance = TMetric::distance(&enr.node_id().raw(), &target.raw())
            .log2()
            .unwrap_or(0) as u16;
        let mut distances = vec![start_distance];
        for difference in 1..=start_distance.min(FIND_NODES_MAX_DISTANCE_DIFFERENCE) {
            distances.push(start_distance - difference);
        }
        for distance in
            (start_distance + 1)..=(start_distance + FIND_NODES_MAX_DISTANCE_DIFFERENCE).min(256)
        {
            distances.push(distance);
        }

        match self
            .send_talk_req(enr, Message::FindNodes(FindNodes { distances }))
            .await?
        {
            Message::Nodes(nodes) => Ok(nodes.enrs.into_iter().map(|enr| enr.0).collect()),
            message => Err(anyhow!(
                "Wrong response type! Expected NODES received: {message:?}"
            )),
        }
    }

    pub async fn send_get_enr(&self, enr: &Enr) -> anyhow::Result<Enr> {
        let _permit = self.outgoing_semaphore.acquire().await?;
        let mut enrs = match self
            .send_talk_req(enr, Message::FindNodes(FindNodes { distances: vec![0] }))
            .await?
        {
            Message::Nodes(nodes) => nodes.enrs,
            message => bail!("Wrong response type! Expected NODES received: {message:?}"),
        };

        if enrs.len() == 1 {
            Ok(enrs.remove(0).0)
        } else {
            bail!("Expected 1 enr, received: {}", enrs.len())
        }
    }

    pub async fn send_find_content(
        &self,
        enr: &Enr,
        content_key: &TContentKey,
    ) -> anyhow::Result<FindContentResult> {
        let _permit = self.outgoing_semaphore.acquire().await?;
        let find_content = FindContent {
            content_key: content_key.to_bytes(),
        };
        let response = match self
            .send_talk_req(enr, Message::FindContent(find_content))
            .await?
        {
            Message::Content(content) => content,
            message => bail!("Wrong response type! Expected CONTENT received: {message:?}"),
        };

        match response {
            Content::ConnectionId(conn_id) => {
                let conn_id = u16::from_be(conn_id);
                let cid = utp_rs::cid::ConnectionId {
                    recv: conn_id,
                    send: conn_id.wrapping_add(1),
                    peer_id: enr.node_id(),
                };

                let mut utp_stream = self
                    .utp_socket
                    .connect_with_cid(cid, Peer::new(UtpPeer(enr.clone())), *UTP_CONN_CFG)
                    .await?;

                let mut buf = Vec::with_capacity(/* 1MB */ 1 << 20);
                utp_stream.read_to_eof(&mut buf).await?;

                Ok(FindContentResult::Content(buf.into()))
            }
            Content::Content(bytes) => Ok(FindContentResult::Content(bytes)),
            Content::Enrs(ssz_enrs) => Ok(FindContentResult::Peers(
                ssz_enrs.into_iter().map(|enr| enr.0).collect(),
            )),
        }
    }
}

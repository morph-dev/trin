use std::{marker::PhantomData, sync::Arc};

use anyhow::{anyhow, bail};
use discv5::{Enr, TalkRequest};
use ethportal_api::types::{
    distance::Distance,
    network::Subnetwork,
    ping_extensions::{
        decode::DecodedExtension,
        extension_types::Extensions,
        extensions::{
            type_0::ClientInfoRadiusCapabilities,
            type_1::BasicRadius,
            type_2::HistoryRadius,
            type_65535::{ErrorCodes, PingError},
        },
    },
    portal_wire::{
        Accept, Content, CustomPayload, FindContent, FindNodes, Message, Nodes, Offer, Ping, Pong,
    },
};
use portalnet::discovery::UtpPeer;
use ssz::{Decode, Encode};
use ssz_types::BitList;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use utp_rs::socket::UtpSocket;

use crate::discovery::Discovery;

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub incoming_talk_request_capacity: usize,
    pub outgoing_talk_request_capacity: usize,
}

pub struct Service<TContentKey, TMetric> {
    subnetwork: Subnetwork,
    discovery: Arc<Discovery>,
    _utp_socket: Arc<UtpSocket<UtpPeer>>,
    _phantom_content_key: PhantomData<TContentKey>,
    _phantom_metric: PhantomData<TMetric>,
}

impl<TContentKey, TMetric> Service<TContentKey, TMetric>
where
    TContentKey: 'static + Send + Sync,
    TMetric: 'static + Send + Sync,
{
    pub fn new(
        subnetwork: Subnetwork,
        discovery: Arc<Discovery>,
        utp_socket: Arc<UtpSocket<UtpPeer>>,
    ) -> Self {
        Self {
            subnetwork,
            discovery,
            _utp_socket: utp_socket,
            _phantom_content_key: PhantomData,
            _phantom_metric: PhantomData,
        }
    }

    pub fn spawn(
        subnetwork: Subnetwork,
        discovery: Arc<Discovery>,
        utp_socket: Arc<UtpSocket<UtpPeer>>,
        config: ServiceConfig,
    ) -> anyhow::Result<Arc<Self>> {
        let service = Arc::new(Self::new(subnetwork, discovery, utp_socket));
        Self::start(&service, &config);
        Ok(service)
    }

    pub fn start(service: &Arc<Self>, config: &ServiceConfig) {
        let subnetwork = service.subnetwork;
        let service = Arc::clone(service);

        let (query_rx, mut query_tx) = mpsc::channel(config.incoming_talk_request_capacity);
        service
            .discovery
            .register_handler(service.subnetwork, query_rx);

        tokio::spawn(async move {
            loop {
                let Some((enr, talk_request)) = query_tx.recv().await else {
                    warn!(subnetwork=%service.subnetwork, "Query channel closed");
                    break;
                };
                if let Err(err) = service.process_incoming_talk_request(enr, talk_request) {
                    error!(%err, "Error processing incoming TALKREQ");
                }
            }
        });
        info!(%subnetwork, "Subnetwork Service started");
    }

    pub fn process_incoming_talk_request(
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

    pub fn process_incoming_ping(&self, _enr: Enr, ping: Ping) -> anyhow::Result<Pong> {
        let local_enr = self.discovery.local_enr();

        let response_payload: CustomPayload =
            match DecodedExtension::decode_extension(ping.payload_type, ping.payload) {
                Ok(DecodedExtension::Capabilities(_)) => ClientInfoRadiusCapabilities::new(
                    Distance::ZERO,
                    vec![
                        Extensions::Capabilities.into(),
                        Extensions::HistoryRadius.into(),
                        Extensions::Error.into(),
                    ],
                )
                .into(),
                Ok(DecodedExtension::BasicRadius(_)) => BasicRadius::new(Distance::ZERO).into(),
                Ok(DecodedExtension::HistoryRadius(_)) => {
                    HistoryRadius::new(Distance::ZERO, 0).into()
                }
                Ok(_) => PingError::new(ErrorCodes::ExtensionNotSupported).into(),
                Err(_err) => PingError::new(ErrorCodes::FailedToDecodePayload).into(),
            };
        Ok(Pong {
            enr_seq: local_enr.seq(),
            payload_type: ping.payload_type,
            payload: response_payload,
        })
    }

    pub fn process_incoming_find_nodes(
        &self,
        _enr: Enr,
        _find_nodes: FindNodes,
    ) -> anyhow::Result<Nodes> {
        Ok(Nodes {
            total: 0,
            enrs: vec![],
        })
    }

    pub fn process_incoming_find_content(
        &self,
        _enr: Enr,
        _find_content: FindContent,
    ) -> anyhow::Result<Content> {
        Ok(Content::Enrs(vec![]))
    }

    pub fn process_incoming_offer(&self, _enr: Enr, offer: Offer) -> anyhow::Result<Accept> {
        let accepted_keys = BitList::with_capacity(offer.content_keys.len())
            .expect("should be able to create Bitlist");
        Ok(Accept {
            connection_id: 0,
            content_keys: accepted_keys,
        })
    }
}

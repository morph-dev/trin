use std::time::{Duration, SystemTime, UNIX_EPOCH};

use discv5::TalkRequest;
use ethportal_api::{
    types::{network::Subnetwork, network_spec::network_spec},
    utils::bytes::{hex_encode, hex_encode_upper},
};
use futures::stream::{select_all, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, trace, warn};

/// Handles for communication between the main event handler and an overlay.
pub struct OverlayHandle {
    /// Dispatch messages to an overlay.
    pub tx: Option<mpsc::UnboundedSender<OverlayRequest>>,
    /// Receive submitted events from an overlay.
    pub rx: Option<broadcast::Receiver<EventEnvelope>>,
}

type OverlayChannels = (
    Option<mpsc::UnboundedSender<OverlayRequest>>,
    Option<broadcast::Receiver<EventEnvelope>>,
);
impl From<OverlayChannels> for OverlayHandle {
    fn from((tx, rx): OverlayChannels) -> Self {
        OverlayHandle { tx, rx }
    }
}

#[derive(Debug)]
/// Messages that can be dispatched to an overlay network.
pub enum OverlayRequest {
    /// A TALK-REQ.
    Talk(TalkRequest),
    /// A forwarded event from another overlay.
    Event(EventEnvelope),
}
impl From<EventEnvelope> for OverlayRequest {
    fn from(event: EventEnvelope) -> Self {
        OverlayRequest::Event(event)
    }
}
impl From<TalkRequest> for OverlayRequest {
    fn from(talk_req: TalkRequest) -> Self {
        OverlayRequest::Talk(talk_req)
    }
}

/// Main handler for portal network events
pub struct PortalnetEvents {
    /// Receive Discv5 talk requests.
    pub talk_req_receiver: mpsc::Receiver<TalkRequest>,
    /// Legacy History network send & receive handles.
    pub legacy_history_handle: OverlayHandle,
    /// State network send & receive handles.
    pub state_handle: OverlayHandle,
    /// Beacon network send & receive handles.
    pub beacon_handle: OverlayHandle,
    /// Send TalkReq events with "utp" protocol id to `UtpListener`
    pub utp_talk_reqs: mpsc::UnboundedSender<TalkRequest>,
}

impl PortalnetEvents {
    pub async fn new(
        talk_req_receiver: mpsc::Receiver<TalkRequest>,
        legacy_history_channels: OverlayChannels,
        state_channels: OverlayChannels,
        beacon_channels: OverlayChannels,
        utp_talk_reqs: mpsc::UnboundedSender<TalkRequest>,
    ) -> Self {
        Self {
            talk_req_receiver,
            legacy_history_handle: legacy_history_channels.into(),
            state_handle: state_channels.into(),
            beacon_handle: beacon_channels.into(),
            utp_talk_reqs,
        }
    }

    /// Main loop to dispatch `Discv5` and uTP events
    pub async fn start(mut self) {
        let mut receivers = vec![];
        if let Some(rx) = self.legacy_history_handle.rx.take() {
            receivers.push(rx);
        }
        if let Some(rx) = self.state_handle.rx.take() {
            receivers.push(rx);
        }
        if let Some(rx) = self.beacon_handle.rx.take() {
            receivers.push(rx);
        }

        if receivers.is_empty() {
            panic!("No networks are available for requests, trin expects at least one");
        }

        let mut event_stream = select_all(receivers.into_iter().map(BroadcastStream::new));
        loop {
            tokio::select! {
                Some(talk_req) = self.talk_req_receiver.recv() => self.dispatch_discv5_talk_req(talk_req),
                Some(event) = event_stream.next() => {
                    match event {
                        Ok(event) => self.dispatch_overlay_event(event),
                        Err(e) => error!(
                            error = %e,
                            "Error reading from event stream"
                        )
                    }
                }
            }
        }
    }

    /// Dispatch Discv5 TalkRequest event to overlay networks or uTP socket
    fn dispatch_discv5_talk_req(&self, request: TalkRequest) {
        let subnetwork = network_spec()
            .get_subnetwork_from_protocol_identifier(&hex_encode_upper(request.protocol()));

        match subnetwork {
            Ok(subnetwork) => match subnetwork {
                Subnetwork::LegacyHistory => self.send_overlay_request(
                    self.legacy_history_handle.tx.as_ref(),
                    request.into(),
                    Subnetwork::LegacyHistory,
                ),
                Subnetwork::Beacon => self.send_overlay_request(
                    self.beacon_handle.tx.as_ref(),
                    request.into(),
                    Subnetwork::Beacon,
                ),
                Subnetwork::State => self.send_overlay_request(
                    self.state_handle.tx.as_ref(),
                    request.into(),
                    Subnetwork::State,
                ),
                Subnetwork::Utp => {
                    if let Err(err) = self.utp_talk_reqs.send(request) {
                        error!(%err, "Error forwarding talk request to uTP socket");
                    }
                }
                _ => {
                    warn!(
                        "Received TalkRequest on unsupported subnetwork from={} protocol={} body={}",
                        request.node_id(),
                        hex_encode_upper(request.protocol()),
                        hex_encode(request.body()),
                    );
                }
            },
            Err(err) => warn!(
                "Received TalkRequest on unknown protocol from={} protocol={} body={} err={err}",
                request.node_id(),
                hex_encode_upper(request.protocol()),
                hex_encode(request.body()),
            ),
        }
    }

    fn dispatch_overlay_event(&self, event: EventEnvelope) {
        use OverlayRequest::Event;

        let all_subnetworks = vec![
            Subnetwork::LegacyHistory,
            Subnetwork::Beacon,
            Subnetwork::State,
        ];
        let mut recipients = event
            .destination
            .as_ref()
            .unwrap_or(&all_subnetworks)
            .to_owned();
        recipients.retain(|id| id != &event.from);

        trace!("Dispatching event {:?} from {} overlay", event, event.from);
        if recipients.is_empty() {
            error!("No valid recipients for this event");
        }

        if recipients.contains(&Subnetwork::Beacon) {
            self.send_overlay_request(
                self.beacon_handle.tx.as_ref(),
                Event(event.clone()),
                Subnetwork::Beacon,
            );
        }
        if recipients.contains(&Subnetwork::State) {
            self.send_overlay_request(
                self.state_handle.tx.as_ref(),
                Event(event.clone()),
                Subnetwork::State,
            );
        }
        if recipients.contains(&Subnetwork::LegacyHistory) {
            self.send_overlay_request(
                self.legacy_history_handle.tx.as_ref(),
                Event(event.clone()),
                Subnetwork::LegacyHistory,
            );
        }
    }

    fn send_overlay_request(
        &self,
        tx: Option<&mpsc::UnboundedSender<OverlayRequest>>,
        msg: OverlayRequest,
        dest: Subnetwork,
    ) {
        match tx {
            Some(tx) => {
                if let Err(err) = tx.send(msg) {
                    error!("Error sending request to {dest} network: {err}");
                }
            }
            None => debug!("Received {dest} request, but {dest} event handler not initialized."),
        };
    }
}

/// Events that can be produced by the `OverlayProtocol` event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayEvent {
    LightClientOptimisticUpdate,
    LightClientFinalityUpdate,
}

/// Timestamp of an overlay event.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Timestamp {
    /// Timestamp not available.
    NotAvailable,
    /// Event creation time.
    CreateTime(i64),
}

impl Timestamp {
    /// Convert the timestamp to milliseconds since epoch.
    pub fn to_millis(self) -> Option<i64> {
        match self {
            Timestamp::NotAvailable | Timestamp::CreateTime(-1) => None,
            Timestamp::CreateTime(t) => Some(t),
        }
    }

    /// Creates a new `Timestamp::CreateTime` representing the current time.
    pub fn now() -> Timestamp {
        Timestamp::from(SystemTime::now())
    }
}

impl From<i64> for Timestamp {
    fn from(system_time: i64) -> Timestamp {
        Timestamp::CreateTime(system_time)
    }
}

impl From<SystemTime> for Timestamp {
    fn from(system_time: SystemTime) -> Timestamp {
        Timestamp::CreateTime(millis_to_epoch(system_time))
    }
}

/// A wrapper around an overlay event that includes additional metadata.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct EventEnvelope {
    /// The timestamp of this event's generation.
    pub timestamp: Timestamp,
    /// The subnetwork that generated this event.
    pub from: Subnetwork,
    /// The event payload.
    pub payload: OverlayEvent,
    /// Specifies the subnetworks to which this event should be sent.
    ///
    /// A value of `None` is taken to indicate `all protocols`.
    pub destination: Option<Vec<Subnetwork>>,
}

impl EventEnvelope {
    pub fn new(
        payload: OverlayEvent,
        from: Subnetwork,
        destination: Option<Vec<Subnetwork>>,
    ) -> Self {
        let timestamp = Timestamp::now();
        Self {
            timestamp,
            from,
            payload,
            destination,
        }
    }
}

/// Converts the given time to the number of milliseconds since the Unix epoch.
pub fn millis_to_epoch(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as i64
}

#[cfg(test)]
mod test {
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn test_timestamp_creation() {
        let now = SystemTime::now();
        let t1 = Timestamp::now();
        let t2 = Timestamp::from(now);
        let expected = Timestamp::CreateTime(millis_to_epoch(now));

        assert_eq!(t2, expected);
        assert!(t1.to_millis().unwrap() - t2.to_millis().unwrap() < 10);
    }

    #[test]
    fn test_timestamp_conversion() {
        assert_eq!(Timestamp::CreateTime(100).to_millis(), Some(100));
        assert_eq!(Timestamp::CreateTime(-1).to_millis(), None);
        assert_eq!(Timestamp::NotAvailable.to_millis(), None);
        let t: Timestamp = 100.into();
        assert_eq!(t, Timestamp::CreateTime(100));
    }
}

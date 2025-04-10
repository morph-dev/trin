use std::{collections::VecDeque, fmt::Debug, time::Instant};

use discv5::{enr::NodeId, Enr};
use ethportal_api::types::distance::{Distance, Metric, XorMetric};

#[derive(Debug, Clone)]
pub struct LivenessCheck {
    pub success: bool,
    pub timestamp: Instant,
}

pub struct Peer {
    enr: Enr,
    radius: Distance,
    reputation: f32,
    liveness_checks: VecDeque<LivenessCheck>,
}

impl Peer {
    /// The maximum number of liveness checks that we store. Value chosen arbitrarily.
    const MAX_LIVENESS_CHECKS: usize = 10;
    /// The peers is considered alive is any of this many recent liveness checks was successful.
    const RECENT_LIVE_CHECKS: usize = 2;

    const REPUTATION_START_VALUE: f32 = 50.;
    const REPUTATION_MIN_VALUE: f32 = 1.;
    const REPUTATION_MAX_VALUE: f32 = 100.;
    const REPUTATION_BOOST: f32 = 1.;
    const REPUTATION_SLASHING_FACTOR: f32 = 0.5;

    pub fn new(enr: Enr) -> Self {
        Self {
            enr,
            radius: Distance::ZERO,
            reputation: Self::REPUTATION_START_VALUE,
            liveness_checks: VecDeque::with_capacity(Self::MAX_LIVENESS_CHECKS + 1),
        }
    }

    pub fn enr(&self) -> Enr {
        self.enr.clone()
    }

    pub fn client(&self) -> String {
        self.enr
            .get_decodable::<String>("c")
            .and_then(|client| client.ok())
            .unwrap_or("(unknown)".to_string())
    }

    pub fn node_id(&self) -> NodeId {
        self.enr.node_id()
    }

    pub fn radius(&self) -> Distance {
        self.radius
    }

    pub fn reputation(&self) -> f32 {
        self.reputation
    }

    pub fn is_alive(&self) -> bool {
        self.iter_liveness_checks()
            .take(Self::RECENT_LIVE_CHECKS)
            .any(|liveness_check| liveness_check.success)
    }

    /// Returns true if content is within radius.
    pub fn is_content_within_radius(&self, content_id: &[u8; 32]) -> bool {
        let distance = XorMetric::distance(&self.enr.node_id().raw(), content_id);
        distance <= self.radius
    }

    /// Returns true if all latest [Self::MAX_LIVENESS_CHECKS] liveness checks failed.
    pub fn is_obsolete(&self) -> bool {
        if self.liveness_checks.len() < Self::MAX_LIVENESS_CHECKS {
            return false;
        }
        self.liveness_checks
            .iter()
            .all(|liveness_check| !liveness_check.success)
    }

    pub fn record_successful_liveness_check(&mut self, enr: Enr, radius: Distance) {
        assert_eq!(
            self.enr.node_id(),
            enr.node_id(),
            "Received enr for different peer. Expected node-id: {}, received enr: {enr}",
            self.enr.node_id(),
        );
        self.record_rpc_result(/* success= */ true);
        if self.enr.seq() <= enr.seq() {
            self.enr = enr;
        }
        self.radius = radius;
        self.liveness_checks.push_front(LivenessCheck {
            success: true,
            timestamp: Instant::now(),
        });
        self.purge();
    }

    pub fn record_failed_liveness_check(&mut self) {
        self.record_rpc_result(/* success= */ false);
        self.liveness_checks.push_front(LivenessCheck {
            success: false,
            timestamp: Instant::now(),
        });
        self.purge();
    }

    pub fn record_rpc_result(&mut self, success: bool) {
        if success {
            self.reputation += Self::REPUTATION_BOOST;
        } else {
            self.reputation *= Self::REPUTATION_SLASHING_FACTOR;
        }
        self.reputation = self
            .reputation
            .clamp(Self::REPUTATION_MIN_VALUE, Self::REPUTATION_MAX_VALUE);
    }

    /// Removes oldest liveness checks and offer events, if we exceeded capacity.
    fn purge(&mut self) {
        if self.liveness_checks.len() > Self::MAX_LIVENESS_CHECKS {
            self.liveness_checks.drain(Self::MAX_LIVENESS_CHECKS..);
        }
        // if self.offer_events.len() > Self::MAX_OFFER_EVENTS {
        //     self.offer_events.drain(Self::MAX_OFFER_EVENTS..);
        // }
    }

    pub fn iter_liveness_checks(&self) -> impl Iterator<Item = &LivenessCheck> {
        self.liveness_checks.iter()
    }
}

impl Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("is_alive", &self.is_alive())
            .field("node_id", &self.node_id())
            .field("reputation", &self.reputation)
            .finish_non_exhaustive()
    }
}

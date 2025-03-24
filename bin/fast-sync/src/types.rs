use alloy::primitives::Bytes;
use discv5::Enr;

#[derive(Debug)]
pub enum FindContentResult {
    Content(Bytes),
    Peers(Vec<Enr>),
}

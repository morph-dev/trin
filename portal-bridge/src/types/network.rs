use std::{fmt, str::FromStr};

/// The different subnetworks that can be used to run the bridge
#[derive(Debug, PartialEq, Clone)]
pub enum NetworkKind {
    Beacon,
    History,
    State,
    Verkle,
}

impl fmt::Display for NetworkKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Beacon => write!(f, "beacon"),
            Self::History => write!(f, "history"),
            Self::State => write!(f, "state"),
            Self::Verkle => write!(f, "verkle"),
        }
    }
}

impl FromStr for NetworkKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "beacon" => Ok(NetworkKind::Beacon),
            "history" => Ok(NetworkKind::History),
            "state" => Ok(NetworkKind::State),
            "verkle" => Ok(NetworkKind::Verkle),
            _ => {
                Err("Invalid network arg. Expected either 'beacon', 'history', 'state' or 'verkle'")
            }
        }
    }
}

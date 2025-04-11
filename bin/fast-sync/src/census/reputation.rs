pub const REPUTATION_START_VALUE: f32 = 50.;
pub const REPUTATION_MIN_VALUE: f32 = 1.;
pub const REPUTATION_MAX_VALUE: f32 = 100.;
pub const REPUTATION_BOOST: f32 = 1.;
pub const REPUTATION_SLASHING_FACTOR: f32 = 0.5;

pub fn update_reputation(reputation: &mut f32, success: bool) {
    if success {
        *reputation += REPUTATION_BOOST;
    } else {
        *reputation *= REPUTATION_SLASHING_FACTOR;
    }
    *reputation = reputation.clamp(REPUTATION_MIN_VALUE, REPUTATION_MAX_VALUE);
}

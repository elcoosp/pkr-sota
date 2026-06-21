pub trait GameRules: Send + Sync {
    fn max_actions_per_node(&self) -> u8;
    fn deck_size(&self) -> usize;
    fn hand_size(&self) -> usize;
}

pub struct InfoSet {
    pub hash: u64,
    pub valid_actions: Vec<u8>,
}

pub struct SotaAdvice {
    pub cdf_probabilities: Vec<u8>, // 0-255 representing 0.0-1.0
}

pub trait BlueprintProvider {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice>;
}

pub trait Evaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u16;
}

pub trait AbstractionBuilder {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8]) -> u64;
}

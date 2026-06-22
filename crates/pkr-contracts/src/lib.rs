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
    pub cdf_probabilities: [u8; 16], // max 16 actions
    pub len: u8,
}

pub trait BlueprintProvider: Send + Sync {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice>;
}

pub trait Evaluator: Send + Sync {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32;
}

pub trait AbstractionBuilder: Send + Sync {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64;
}

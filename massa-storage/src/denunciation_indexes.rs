use massa_models::denunciation::Denunciation;
use massa_models::{denunciation::DenunciationId, prehash::PreHashMap};

/// Container for all endorsements and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct DenunciationIndexes {
    /// Denunciations structure container
    denunciations: PreHashMap<DenunciationId, Denunciation>,
}

impl DenunciationIndexes {
    /// Insert a denunciation
    /// Arguments:
    /// - denunciation: the denunciation to insert
    pub(crate) fn insert(&mut self, denunciation: Denunciation) {
        let denunciation_id = DenunciationId::from(&denunciation);
        self.denunciations.try_insert(denunciation_id, denunciation);
    }

    /// Remove a endorsement, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// * `endorsement_id`: the endorsement id to remove
    pub(crate) fn remove(&mut self, denunciation_id: &DenunciationId) -> Option<Denunciation> {
        self.denunciations.remove(denunciation_id)
    }

    /// Gets a reference to a stored denunciation, if any.
    pub fn get(&self, id: &DenunciationId) -> Option<&Denunciation> {
        self.denunciations.get(id)
    }

    /// Checks whether an denunciation exists in global storage.
    pub fn contains(&self, id: &DenunciationId) -> bool {
        self.denunciations.contains_key(id)
    }
}

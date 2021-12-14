use massa_models::{DeserializeCompact, SerializeCompact, Slot};

use crate::sce_ledger::SCELedger;

#[derive(Debug, Clone)]
pub struct BootstrapExecutionState {
    pub final_ledger: SCELedger,
    pub final_slot: Slot,
}

impl SerializeCompact for BootstrapExecutionState {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // final ledger
        res.extend(self.final_ledger.to_bytes_compact()?);

        // final slot
        res.extend(self.final_slot.to_bytes_compact()?);

        Ok(res)
    }
}

impl DeserializeCompact for BootstrapExecutionState {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // final ledger
        let (final_ledger, delta) = SCELedger::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // final slot
        let (final_slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            BootstrapExecutionState {
                final_ledger,
                final_slot,
            },
            cursor,
        ))
    }
}

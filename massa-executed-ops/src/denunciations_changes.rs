//! Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_models::denunciation::DenunciationIndex;
use std::collections::HashSet;

/// Speculative changes for ExecutedOps
pub type ExecutedDenunciationsChanges = HashSet<DenunciationIndex>;

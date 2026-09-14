//! Logistic calibration constants for the `calibrated` merge
//! strategy. Extracted from settings.rs for file-size compliance.

use serde::{Deserialize, Serialize};

/// Logistic calibration constants for one channel: `p = 1/(1 +
/// exp(-(strength - mid) / width))`. `mid` is the strength at which a
/// channel is judged 50/50 distinctive; `width` controls how sharp
/// the transition is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelCalibration {
    pub mid: f64,
    pub width: f64,
}

/// Calibration constants per channel. Defaults fitted on the CogZ
/// self-corpus: code negatives ≤0.29 vs real ≥0.26 (overlapping —
/// gate stays the primary silence mechanism), knowledge negatives
/// ≤0.63 vs real ≥0.67.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationConfig {
    pub code: ChannelCalibration,
    pub knowledge: ChannelCalibration,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            code: ChannelCalibration {
                mid: 0.32,
                width: 0.04,
            },
            knowledge: ChannelCalibration {
                mid: 0.64,
                width: 0.03,
            },
        }
    }
}

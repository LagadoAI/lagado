//! validator.rs — Pre-execution action validation.
//!
//! Checks actions against policy before they reach the OS.
//! Ties into gate.rs risk tiers: Read→pass, Write→log, Destructive→block unless HITL confirmed.

use crate::operator::RiskLevel;

pub struct Validator;

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Allow,
    Log,    // allow but record
    Block { reason: String },
}

impl Validator {
    pub fn new() -> Self { Self }

    /// Validate an action given its risk level and whether HITL already confirmed it.
    pub fn validate(&self, risk: RiskLevel, hitl_confirmed: bool) -> ValidationResult {
        match risk {
            RiskLevel::Read => ValidationResult::Log,
            RiskLevel::Write => {
                if hitl_confirmed {
                    ValidationResult::Allow
                } else {
                    ValidationResult::Block {
                        reason: "Write action requires HITL confirmation".into(),
                    }
                }
            }
            RiskLevel::Destructive => {
                if hitl_confirmed {
                    ValidationResult::Log
                } else {
                    ValidationResult::Block {
                        reason: "Destructive action requires typed HITL confirmation".into(),
                    }
                }
            }
        }
    }
}

use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;

/// Validate whether a state transition from `from` to `to` is permitted.
///
/// Returns `Ok(())` for valid transitions (including identity transitions where
/// `from == to`). Returns `Err(CcxError::InvalidStateTransition)` for any
/// transition not listed in the allow-list below.
pub fn validate_transition(
    from: WorkExecutionState,
    to: WorkExecutionState,
) -> Result<(), CcxError> {
    if from == to {
        return Ok(());
    }

    let is_valid = match (from, to) {
        // Normal flow
        (WorkExecutionState::Created, WorkExecutionState::TaskFileCreated) => true,
        (WorkExecutionState::TaskFileCreated, WorkExecutionState::Dispatched) => true,
        (WorkExecutionState::Dispatched, WorkExecutionState::Running) => true,
        (WorkExecutionState::Running, WorkExecutionState::PrOpen) => true,
        (WorkExecutionState::PrOpen, WorkExecutionState::GateCheck) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::ReviewFixing) => true,
        (WorkExecutionState::ReviewFixing, WorkExecutionState::GateCheck) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::MergeReady) => true,
        (WorkExecutionState::MergeReady, WorkExecutionState::Merging) => true,
        (WorkExecutionState::Merging, WorkExecutionState::Merged) => true,

        // Post-merge follow-up.
        // FollowupRequired is semi-terminal: no further outbound transitions are
        // defined in spec section 3.2. The only exits are the (_, Hold) / (_, Canceled)
        // wildcards below, which apply to all states.
        (WorkExecutionState::Merged, WorkExecutionState::FollowupRequired) => true,

        // Return flow
        (WorkExecutionState::Running, WorkExecutionState::Returned) => true,
        (WorkExecutionState::Returned, WorkExecutionState::Superseded) => true,
        (WorkExecutionState::Returned, WorkExecutionState::TaskFileCreated) => true,

        // Interruptions and failures
        (WorkExecutionState::Running, WorkExecutionState::Blocked) => true,
        (WorkExecutionState::Running, WorkExecutionState::Failed) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::Failed) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::Blocked) => true,
        (WorkExecutionState::Merging, WorkExecutionState::GateCheck) => true,
        (WorkExecutionState::Merging, WorkExecutionState::ReviewFixing) => true,
        (WorkExecutionState::ReviewFixing, WorkExecutionState::Blocked) => true,

        // Any state → Hold or Canceled
        (_, WorkExecutionState::Hold) => true,
        (_, WorkExecutionState::Canceled) => true,

        // Retry / resume transitions
        (WorkExecutionState::Failed, WorkExecutionState::Dispatched) => true,
        (WorkExecutionState::Failed, WorkExecutionState::TaskFileCreated) => true,
        (WorkExecutionState::Blocked, WorkExecutionState::Running) => true,
        (WorkExecutionState::Hold, WorkExecutionState::Dispatched) => true,
        (WorkExecutionState::Hold, WorkExecutionState::Running) => true,
        (WorkExecutionState::Hold, WorkExecutionState::TaskFileCreated) => true,

        _ => false,
    };

    if is_valid {
        Ok(())
    } else {
        Err(CcxError::InvalidStateTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use WorkExecutionState::*;

    #[test]
    fn identity_is_always_valid() {
        for state in [
            Created, TaskFileCreated, Dispatched, Running, PrOpen, GateCheck,
            ReviewFixing, MergeReady, Merging, Merged, FollowupRequired,
            Returned, Blocked, Failed, Hold, Canceled, Superseded,
        ] {
            assert!(validate_transition(state, state).is_ok(), "{state} → {state} should be valid");
        }
    }

    #[test]
    fn normal_flow_is_valid() {
        let chain = [
            (Created, TaskFileCreated),
            (TaskFileCreated, Dispatched),
            (Dispatched, Running),
            (Running, PrOpen),
            (PrOpen, GateCheck),
            (GateCheck, ReviewFixing),
            (ReviewFixing, GateCheck),
            (GateCheck, MergeReady),
            (MergeReady, Merging),
            (Merging, Merged),
            (Merged, FollowupRequired),
        ];
        for (from, to) in chain {
            assert!(validate_transition(from, to).is_ok(), "{from} → {to} should be valid");
        }
    }

    #[test]
    fn any_state_can_transition_to_hold_or_canceled() {
        for from in [
            Created, TaskFileCreated, Dispatched, Running, PrOpen, GateCheck,
            ReviewFixing, MergeReady, Merging, Merged, FollowupRequired,
            Returned, Blocked, Failed, Superseded,
        ] {
            assert!(validate_transition(from, Hold).is_ok(), "{from} → Hold");
            assert!(validate_transition(from, Canceled).is_ok(), "{from} → Canceled");
        }
        // Cross-transitions between the two interrupt states
        assert!(validate_transition(Canceled, Hold).is_ok(), "Canceled → Hold");
        assert!(validate_transition(Hold, Canceled).is_ok(), "Hold → Canceled");
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let invalid = [
            (Created, Running),
            (Merged, Running),
            (Canceled, Running),
            (Dispatched, Merged),
            (PrOpen, Running),
        ];
        for (from, to) in invalid {
            let result = validate_transition(from, to);
            assert!(result.is_err(), "{from} → {to} should be invalid");
            assert!(
                matches!(result.unwrap_err(), CcxError::InvalidStateTransition { .. }),
                "expected InvalidStateTransition"
            );
        }
    }

    #[test]
    fn retry_and_resume_flows_are_valid() {
        let transitions = [
            (Failed, Dispatched),
            (Failed, TaskFileCreated),
            (Blocked, Running),
            (Hold, Dispatched),
            (Hold, Running),
            (Hold, TaskFileCreated),
        ];
        for (from, to) in transitions {
            assert!(validate_transition(from, to).is_ok(), "{from} → {to} should be valid");
        }
    }
}

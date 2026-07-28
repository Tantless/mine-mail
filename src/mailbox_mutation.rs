use crate::models::{
    MessageActionKind, MessageMutationErrorKind, MutationStatus, RemoteMutationPhase,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StrongIdentityMatch {
    None,
    Unique,
    Ambiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceIdentityState {
    Exact,
    Missing,
    Mismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MutationDecision {
    Execute,
    Confirm,
    Stop {
        status: MutationStatus,
        error_kind: MessageMutationErrorKind,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PersistedPhaseWork {
    Transfer,
    SourceDelete,
    Finalize,
    Reconcile,
    Done,
    Stop {
        status: MutationStatus,
        error_kind: MessageMutationErrorKind,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PersistedFlagWork {
    Execute,
    Reconcile,
    Done,
}

pub(crate) fn persisted_flag_work(status: MutationStatus) -> PersistedFlagWork {
    match status {
        MutationStatus::Pending => PersistedFlagWork::Execute,
        MutationStatus::InFlight
        | MutationStatus::NeedsAttention
        | MutationStatus::OutcomeUnknown => PersistedFlagWork::Reconcile,
        MutationStatus::Confirmed => PersistedFlagWork::Done,
    }
}

/// Maps durable action state to the only safe next class of work.
///
/// In particular, a persisted `*_started` phase is never interpreted as
/// permission to repeat a transfer. The worker must first reconcile the exact
/// source epoch and, for an uncertain transfer, the strong destination
/// identity.
pub(crate) fn persisted_phase_work(
    kind: MessageActionKind,
    status: MutationStatus,
    phase: RemoteMutationPhase,
) -> PersistedPhaseWork {
    if status == MutationStatus::Confirmed {
        return PersistedPhaseWork::Done;
    }
    if matches!(
        status,
        MutationStatus::NeedsAttention | MutationStatus::OutcomeUnknown
    ) {
        return PersistedPhaseWork::Reconcile;
    }

    match (kind, phase) {
        (
            MessageActionKind::Archive | MessageActionKind::MoveToTrash,
            RemoteMutationPhase::Queued,
        ) => PersistedPhaseWork::Transfer,
        (MessageActionKind::PermanentDelete, RemoteMutationPhase::Queued) => {
            PersistedPhaseWork::SourceDelete
        }
        (_, RemoteMutationPhase::TransferStarted | RemoteMutationPhase::SourceDeleteStarted) => {
            PersistedPhaseWork::Reconcile
        }
        (
            MessageActionKind::Archive | MessageActionKind::MoveToTrash,
            RemoteMutationPhase::TransferAcknowledged,
        ) => PersistedPhaseWork::SourceDelete,
        (_, RemoteMutationPhase::SourceDeleteAcknowledged) => PersistedPhaseWork::Finalize,
        (MessageActionKind::PermanentDelete, RemoteMutationPhase::TransferAcknowledged) => {
            PersistedPhaseWork::Stop {
                status: MutationStatus::NeedsAttention,
                error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
            }
        }
    }
}

pub(crate) fn decide_preflight(
    expected_uid_validity: u32,
    selected_uid_validity: Option<u32>,
    source: SourceIdentityState,
    destination: StrongIdentityMatch,
) -> MutationDecision {
    if selected_uid_validity != Some(expected_uid_validity) {
        return match destination {
            StrongIdentityMatch::Unique => MutationDecision::Confirm,
            StrongIdentityMatch::Ambiguous => MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
            },
            StrongIdentityMatch::None => MutationDecision::Stop {
                status: MutationStatus::NeedsAttention,
                error_kind: MessageMutationErrorKind::UidValidityChanged,
            },
        };
    }

    match source {
        SourceIdentityState::Exact => match destination {
            StrongIdentityMatch::None => MutationDecision::Execute,
            StrongIdentityMatch::Unique | StrongIdentityMatch::Ambiguous => {
                MutationDecision::Stop {
                    status: MutationStatus::OutcomeUnknown,
                    error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
                }
            }
        },
        SourceIdentityState::Missing => match destination {
            StrongIdentityMatch::Unique => MutationDecision::Confirm,
            StrongIdentityMatch::Ambiguous => MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
            },
            StrongIdentityMatch::None => MutationDecision::Stop {
                status: MutationStatus::NeedsAttention,
                error_kind: MessageMutationErrorKind::SourceMissing,
            },
        },
        SourceIdentityState::Mismatch => MutationDecision::Stop {
            status: MutationStatus::NeedsAttention,
            error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
        },
    }
}

pub(crate) fn decide_after_transfer(destination: StrongIdentityMatch) -> MutationDecision {
    match destination {
        StrongIdentityMatch::Unique => MutationDecision::Confirm,
        StrongIdentityMatch::None | StrongIdentityMatch::Ambiguous => MutationDecision::Stop {
            status: MutationStatus::OutcomeUnknown,
            error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
        },
    }
}

pub(crate) fn decide_copy_source_deletion(
    expected_uid_validity: u32,
    selected_uid_validity: Option<u32>,
    source: SourceIdentityState,
    destination: StrongIdentityMatch,
) -> MutationDecision {
    if selected_uid_validity == Some(expected_uid_validity)
        && source == SourceIdentityState::Exact
        && destination == StrongIdentityMatch::Unique
    {
        MutationDecision::Execute
    } else {
        MutationDecision::Stop {
            status: MutationStatus::OutcomeUnknown,
            error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
        }
    }
}

pub(crate) fn command_outcome_unknown(network_unavailable: bool) -> MutationDecision {
    MutationDecision::Stop {
        status: MutationStatus::OutcomeUnknown,
        error_kind: if network_unavailable {
            MessageMutationErrorKind::NetworkUnavailable
        } else {
            MessageMutationErrorKind::Unknown
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MutationDecision, PersistedFlagWork, PersistedPhaseWork, SourceIdentityState,
        StrongIdentityMatch, command_outcome_unknown, decide_after_transfer,
        decide_copy_source_deletion, decide_preflight, persisted_flag_work, persisted_phase_work,
    };
    use crate::models::{
        MessageActionKind, MessageMutationErrorKind, MutationStatus, RemoteMutationPhase,
    };

    #[test]
    fn current_epoch_and_exact_source_is_the_only_normal_execute_path() {
        assert_eq!(
            decide_preflight(
                7,
                Some(7),
                SourceIdentityState::Exact,
                StrongIdentityMatch::None,
            ),
            MutationDecision::Execute
        );
        assert!(matches!(
            decide_preflight(
                7,
                Some(7),
                SourceIdentityState::Exact,
                StrongIdentityMatch::Unique,
            ),
            MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                ..
            }
        ));
        assert_eq!(
            decide_preflight(
                7,
                Some(8),
                SourceIdentityState::Exact,
                StrongIdentityMatch::None,
            ),
            MutationDecision::Stop {
                status: MutationStatus::NeedsAttention,
                error_kind: MessageMutationErrorKind::UidValidityChanged,
            }
        );
    }

    #[test]
    fn vanished_source_confirms_only_one_strong_destination_match() {
        assert_eq!(
            decide_preflight(
                7,
                Some(7),
                SourceIdentityState::Missing,
                StrongIdentityMatch::Unique,
            ),
            MutationDecision::Confirm
        );
        assert_eq!(
            decide_preflight(
                7,
                Some(7),
                SourceIdentityState::Missing,
                StrongIdentityMatch::Ambiguous,
            ),
            MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                error_kind: MessageMutationErrorKind::AmbiguousRemoteState,
            }
        );
    }

    #[test]
    fn post_transfer_without_one_unique_destination_never_confirms() {
        assert_eq!(
            decide_after_transfer(StrongIdentityMatch::Unique),
            MutationDecision::Confirm
        );
        assert!(matches!(
            decide_after_transfer(StrongIdentityMatch::None),
            MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                ..
            }
        ));
        assert!(matches!(
            decide_after_transfer(StrongIdentityMatch::Ambiguous),
            MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                ..
            }
        ));
    }

    #[test]
    fn copy_deletes_source_only_after_both_identity_checks_in_same_epoch() {
        assert_eq!(
            decide_copy_source_deletion(
                11,
                Some(11),
                SourceIdentityState::Exact,
                StrongIdentityMatch::Unique,
            ),
            MutationDecision::Execute
        );
        for decision in [
            decide_copy_source_deletion(
                11,
                Some(12),
                SourceIdentityState::Exact,
                StrongIdentityMatch::Unique,
            ),
            decide_copy_source_deletion(
                11,
                Some(11),
                SourceIdentityState::Missing,
                StrongIdentityMatch::Unique,
            ),
            decide_copy_source_deletion(
                11,
                Some(11),
                SourceIdentityState::Exact,
                StrongIdentityMatch::Ambiguous,
            ),
        ] {
            assert!(matches!(
                decision,
                MutationDecision::Stop {
                    status: MutationStatus::OutcomeUnknown,
                    ..
                }
            ));
        }
    }

    #[test]
    fn an_uncertain_command_is_never_returned_to_pending() {
        assert_eq!(
            command_outcome_unknown(true),
            MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                error_kind: MessageMutationErrorKind::NetworkUnavailable,
            }
        );
        assert_eq!(
            command_outcome_unknown(false),
            MutationDecision::Stop {
                status: MutationStatus::OutcomeUnknown,
                error_kind: MessageMutationErrorKind::Unknown,
            }
        );
    }

    #[test]
    fn a_started_transfer_always_reconciles_before_more_network_work() {
        for status in [MutationStatus::Pending, MutationStatus::InFlight] {
            assert_eq!(
                persisted_phase_work(
                    MessageActionKind::Archive,
                    status,
                    RemoteMutationPhase::TransferStarted,
                ),
                PersistedPhaseWork::Reconcile
            );
        }
        assert_eq!(
            persisted_phase_work(
                MessageActionKind::MoveToTrash,
                MutationStatus::OutcomeUnknown,
                RemoteMutationPhase::Queued,
            ),
            PersistedPhaseWork::Reconcile
        );
    }

    #[test]
    fn only_a_fresh_move_enters_transfer_and_permanent_delete_never_does() {
        assert_eq!(
            persisted_phase_work(
                MessageActionKind::MoveToTrash,
                MutationStatus::InFlight,
                RemoteMutationPhase::Queued,
            ),
            PersistedPhaseWork::Transfer
        );
        assert_eq!(
            persisted_phase_work(
                MessageActionKind::PermanentDelete,
                MutationStatus::InFlight,
                RemoteMutationPhase::Queued,
            ),
            PersistedPhaseWork::SourceDelete
        );
    }

    #[test]
    fn acknowledged_copy_resumes_deletion_without_recopying() {
        assert_eq!(
            persisted_phase_work(
                MessageActionKind::Archive,
                MutationStatus::InFlight,
                RemoteMutationPhase::TransferAcknowledged,
            ),
            PersistedPhaseWork::SourceDelete
        );
        assert_eq!(
            persisted_phase_work(
                MessageActionKind::Archive,
                MutationStatus::InFlight,
                RemoteMutationPhase::SourceDeleteAcknowledged,
            ),
            PersistedPhaseWork::Finalize
        );
    }

    #[test]
    fn interrupted_flags_reconcile_before_the_idempotent_retry_is_requeued() {
        assert_eq!(
            persisted_flag_work(MutationStatus::Pending),
            PersistedFlagWork::Execute
        );
        for status in [
            MutationStatus::InFlight,
            MutationStatus::NeedsAttention,
            MutationStatus::OutcomeUnknown,
        ] {
            assert_eq!(persisted_flag_work(status), PersistedFlagWork::Reconcile);
        }
        assert_eq!(
            persisted_flag_work(MutationStatus::Confirmed),
            PersistedFlagWork::Done
        );
    }
}

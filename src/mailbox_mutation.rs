use crate::models::{
    MessageActionKind, MessageMutationErrorKind, MutationStatus, RemoteMutationPhase,
};

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
            MessageActionKind::Archive
            | MessageActionKind::MoveToInbox
            | MessageActionKind::MoveToTrash,
            RemoteMutationPhase::Queued,
        ) => PersistedPhaseWork::Transfer,
        (MessageActionKind::PermanentDelete, RemoteMutationPhase::Queued) => {
            PersistedPhaseWork::SourceDelete
        }
        (_, RemoteMutationPhase::TransferStarted | RemoteMutationPhase::SourceDeleteStarted) => {
            PersistedPhaseWork::Reconcile
        }
        (
            MessageActionKind::Archive
            | MessageActionKind::MoveToInbox
            | MessageActionKind::MoveToTrash,
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

#[cfg(test)]
mod tests {
    use super::{PersistedFlagWork, PersistedPhaseWork, persisted_flag_work, persisted_phase_work};
    use crate::models::{MessageActionKind, MutationStatus, RemoteMutationPhase};

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
                MessageActionKind::MoveToInbox,
                MutationStatus::InFlight,
                RemoteMutationPhase::Queued,
            ),
            PersistedPhaseWork::Transfer
        );
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

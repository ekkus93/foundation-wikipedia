//! Conservative pack trust labels until cryptographic publisher verification exists.
//!
//! A manifest declaring Origin::Official is merely a claim. In particular,
//! a matching object digest never establishes publisher identity.

use crate::manifest::{Manifest, Origin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrityState {
    NotChecked,
    Passed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustStatus {
    /// Reserved for a future pinned-key signature verifier; not emitted yet.
    VerifiedPublisher,
    CustomIntegrityVerified,
    NotYetChecked,
    VerificationFailed,
}

impl TrustStatus {
    /// The same accessible text can be used in list cards, details and dialogs.
    pub const fn label(self) -> &'static str {
        match self {
            Self::VerifiedPublisher => "Verified publisher",
            Self::CustomIntegrityVerified => "Custom — integrity verified",
            Self::NotYetChecked => "Not yet checked",
            Self::VerificationFailed => "Verification failed",
        }
    }
}

/// Show structural failures immediately, and never promote unsigned official
/// claims to trusted status based on self-declared hashes or integrity alone.
///
/// IntegrityState::Passed must be supplied only by a complete byte verifier.
/// This does not authorize installation; that requires its own verified
/// object-byte gate, user confirmation for custom packs, and signed origin
/// verification for official publisher packs.
pub fn classify(manifest: &Manifest, integrity: IntegrityState) -> TrustStatus {
    if manifest.validate().is_err() || integrity == IntegrityState::Failed {
        return TrustStatus::VerificationFailed;
    }
    match (&manifest.origin, integrity) {
        (Origin::Custom { .. }, IntegrityState::Passed) => TrustStatus::CustomIntegrityVerified,
        _ => TrustStatus::NotYetChecked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Object, FORMAT_VERSION};

    fn pack(origin: Origin) -> Manifest {
        Manifest {
            version: FORMAT_VERSION,
            pack_id: "physics".into(),
            project: "enwiki".into(),
            snapshot: "20261009".into(),
            origin,
            objects: vec![Object {
                path: "articles/42.cbor.zst".into(),
                sha256: "a".repeat(64),
                bytes: 1024,
            }],
        }
    }

    #[test]
    fn official_claim_and_checksum_can_never_confer_publisher_trust() {
        let claimed = pack(Origin::Official {
            publisher: "Foundation".into(),
        });
        assert_eq!(
            classify(&claimed, IntegrityState::NotChecked),
            TrustStatus::NotYetChecked
        );
        assert_eq!(
            classify(&claimed, IntegrityState::Passed),
            TrustStatus::NotYetChecked
        );
        assert_ne!(
            classify(&claimed, IntegrityState::Passed),
            TrustStatus::VerifiedPublisher
        );
        assert_eq!(
            classify(&claimed, IntegrityState::Failed),
            TrustStatus::VerificationFailed
        );
    }

    #[test]
    fn unsigned_custom_integrity_is_not_publisher_identity() {
        let custom = pack(Origin::Custom {
            definition_id: "physics".into(),
        });
        assert_eq!(
            classify(&custom, IntegrityState::NotChecked),
            TrustStatus::NotYetChecked
        );
        assert_eq!(
            classify(&custom, IntegrityState::Passed),
            TrustStatus::CustomIntegrityVerified
        );
        assert_eq!(
            classify(&custom, IntegrityState::Failed),
            TrustStatus::VerificationFailed
        );
        assert_eq!(
            TrustStatus::CustomIntegrityVerified.label(),
            "Custom — integrity verified"
        );
    }

    #[test]
    fn invalid_manifest_never_shows_success_or_pending() {
        let mut custom = pack(Origin::Custom {
            definition_id: "personal".into(),
        });
        custom.objects[0].path = "../escape".into();
        assert_eq!(
            classify(&custom, IntegrityState::Passed),
            TrustStatus::VerificationFailed
        );
        assert_eq!(
            classify(&custom, IntegrityState::NotChecked),
            TrustStatus::VerificationFailed
        );
    }

    #[test]
    fn all_states_have_explicit_accessible_labels() {
        for status in [
            TrustStatus::VerifiedPublisher,
            TrustStatus::CustomIntegrityVerified,
            TrustStatus::NotYetChecked,
            TrustStatus::VerificationFailed,
        ] {
            assert!(!status.label().trim().is_empty());
        }
    }
}

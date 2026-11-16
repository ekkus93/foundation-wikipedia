//! Conservative pack trust labels.
//!
//! A manifest declaring Origin::Official is merely a claim. In particular,
//! a matching object digest never establishes publisher identity: only the
//! installer's `install_official_pack*`, after `signature::verify` against
//! pinned keys (TRUST-001, ADR 0008), labels a pack "Verified publisher".

use crate::manifest::{Manifest, Origin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrityState {
    NotChecked,
    Passed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustStatus {
    /// Only after a pinned-key publisher signature verified (ADR 0008);
    /// `classify` never returns it.
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

    /// A plain-language explanation for screen readers and pack details.
    /// Only `VerifiedPublisher` mentions the publisher; no other status may
    /// suggest where a pack came from.
    pub const fn description(self) -> &'static str {
        match self {
            Self::VerifiedPublisher => {
                "Signed by the official publisher, and every file matches that signature."
            }
            Self::CustomIntegrityVerified => {
                "Every file is intact, but this pack is not from a verified publisher. \
                 Use it only if you trust where it came from."
            }
            Self::NotYetChecked => "This pack has not been checked yet. Do not rely on it.",
            Self::VerificationFailed => {
                "This pack is damaged, was modified, or its publisher key is no longer \
                 trusted. It cannot be used."
            }
        }
    }

    /// All statuses, for exhaustive presentation tests.
    pub const ALL: [Self; 4] = [
        Self::VerifiedPublisher,
        Self::CustomIntegrityVerified,
        Self::NotYetChecked,
        Self::VerificationFailed,
    ];

    /// Parse a label the installer recorded for an installed pack. Only
    /// the two statuses an install can produce are accepted; anything else
    /// means the record was altered, so the caller must treat it as failed.
    pub fn from_installed_label(label: &str) -> Option<Self> {
        [Self::VerifiedPublisher, Self::CustomIntegrityVerified]
            .into_iter()
            .find(|status| status.label() == label)
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

/// Verify every object byte before reporting a custom pack as integrity-checked.
/// An unsigned official-origin claim remains unverified even after this succeeds.
/// This is read-only; installation requires separate staging and policy checks.
pub fn verify_and_classify<R: std::io::Read>(
    input: &mut R,
) -> Result<(Manifest, TrustStatus), crate::container::ContainerError> {
    let manifest = crate::container::verify_container(input)?;
    let status = classify(&manifest, IntegrityState::Passed);
    Ok((manifest, status))
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
            metadata: crate::manifest::PackMetadata::synthetic_fixture(),
            objects: vec![Object {
                path: "articles/42.cbor.zst".into(),
                sha256: "a".repeat(64),
                bytes: 1024,
            }],
        }
    }

    #[test]
    fn streamed_integrity_is_required_before_custom_trust_label() {
        let payload = b"verified article";
        let mut custom = pack(Origin::Custom {
            definition_id: "physics".into(),
        });
        custom.objects[0].bytes = payload.len() as u64;
        custom.objects[0].sha256 = crate::sha256_hex(payload);
        let mut encoded = Vec::new();
        crate::container::write_container(&mut encoded, &custom, |_, output| {
            output.write_all(payload)
        })
        .unwrap();
        let (decoded, status) = verify_and_classify(&mut encoded.as_slice()).unwrap();
        assert_eq!(decoded, custom);
        assert_eq!(status, TrustStatus::CustomIntegrityVerified);

        let mut forged = encoded.clone();
        *forged.last_mut().unwrap() ^= 1;
        assert!(verify_and_classify(&mut forged.as_slice()).is_err());

        let mut official = custom;
        official.origin = Origin::Official {
            publisher: "unverified".into(),
        };
        let mut encoded = Vec::new();
        crate::container::write_container(&mut encoded, &official, |_, output| {
            output.write_all(payload)
        })
        .unwrap();
        let (_, status) = verify_and_classify(&mut encoded.as_slice()).unwrap();
        assert_eq!(status, TrustStatus::NotYetChecked);
    }

    #[test]
    fn statuses_have_distinct_labels_and_descriptions() {
        let labels: std::collections::HashSet<_> =
            TrustStatus::ALL.iter().map(|s| s.label()).collect();
        let descriptions: std::collections::HashSet<_> =
            TrustStatus::ALL.iter().map(|s| s.description()).collect();
        assert_eq!(labels.len(), 4);
        assert_eq!(descriptions.len(), 4);
        for status in TrustStatus::ALL {
            let publisher_claim = status.description().contains("official publisher");
            assert_eq!(publisher_claim, status == TrustStatus::VerifiedPublisher);
        }
    }

    #[test]
    fn only_installable_statuses_parse_from_installed_records() {
        assert_eq!(
            TrustStatus::from_installed_label("Verified publisher"),
            Some(TrustStatus::VerifiedPublisher)
        );
        assert_eq!(
            TrustStatus::from_installed_label("Custom — integrity verified"),
            Some(TrustStatus::CustomIntegrityVerified)
        );
        for other in [
            "Not yet checked",
            "Verification failed",
            "",
            "verified publisher",
        ] {
            assert_eq!(TrustStatus::from_installed_label(other), None);
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

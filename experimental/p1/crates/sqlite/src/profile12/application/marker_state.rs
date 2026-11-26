//! F2 D/F classifier only. Values in these tests are not native file evidence.
type Identity = [u8; 12];

#[derive(Debug, PartialEq, Eq)]
pub(super) enum State {
    Unpublished,
    Published,
    Recorded,
    Settled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    Integrity,
    StorageUnknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Ledger {
    pub(super) generation: i64,
    pub(super) identity: Option<Identity>,
    pub(super) observation: Option<i64>,
    pub(super) receipt: Option<i64>,
}
pub(super) enum FileView<'a> {
    Absent,
    Present { bytes: &'a [u8], identity: Identity },
    Unknown,
}

// Work/operation/canonical bindings are validated by the SQL loader before it
// constructs Ledger. Owner trust is an admission fact, never a request flag.
pub(super) fn classify(
    ledger: &Ledger,
    expected: &[u8],
    file: FileView<'_>,
) -> Result<State, Fault> {
    if ledger.generation <= 0
        || ledger.observation.is_some_and(|g| g != ledger.generation)
        || ledger.receipt.is_some_and(|g| g != ledger.generation)
        || (ledger.observation.is_some() && ledger.identity.is_none())
        || (ledger.receipt.is_some() && (ledger.identity.is_none() || ledger.observation.is_none()))
    {
        return Err(Fault::Integrity);
    }
    match file {
        FileView::Unknown => Err(Fault::StorageUnknown),
        FileView::Absent => {
            if ledger.identity.is_none() && ledger.observation.is_none() && ledger.receipt.is_none()
            {
                Ok(State::Unpublished)
            } else {
                Err(Fault::Integrity)
            }
        }
        FileView::Present { bytes, identity } => {
            if bytes != expected || ledger.identity.is_some_and(|id| id != identity) {
                return Err(Fault::Integrity);
            }
            Ok(if ledger.receipt.is_some() {
                State::Settled
            } else if ledger.identity.is_some() {
                State::Recorded
            } else {
                State::Published
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn f2_df_matrix_preserves_all_legal_states_and_rejects_invalid_ledgers() {
        let mut checked = 0;
        for identity in [None, Some([1; 12])] {
            for observation in [None, Some(1), Some(2)] {
                for receipt in [None, Some(1), Some(2)] {
                    let d = Ledger {
                        generation: 1,
                        identity,
                        observation,
                        receipt,
                    };
                    let invalid = observation == Some(2)
                        || receipt == Some(2)
                        || (observation.is_some() && identity.is_none())
                        || (receipt.is_some() && (observation.is_none() || identity.is_none()));
                    let present = classify(
                        &d,
                        b"marker",
                        FileView::Present {
                            bytes: b"marker",
                            identity: [1; 12],
                        },
                    );
                    let absent = classify(&d, b"marker", FileView::Absent);
                    let expected = if invalid {
                        Err(Fault::Integrity)
                    } else if receipt.is_some() {
                        Ok(State::Settled)
                    } else if identity.is_some() {
                        Ok(State::Recorded)
                    } else {
                        Ok(State::Published)
                    };
                    assert_eq!(present, expected);
                    assert_eq!(
                        absent,
                        if identity.is_none() && observation.is_none() && receipt.is_none() {
                            Ok(State::Unpublished)
                        } else {
                            Err(Fault::Integrity)
                        }
                    );
                    assert!(classify(&d, b"marker", FileView::Unknown).is_err());
                    checked += 1;
                }
            }
        }
        println!("S3_DF combinations={checked} native_evidence=false side_effects=0");
    }
    #[test]
    fn settled_receipt_never_exempts_missing_replaced_or_corrupt_file() {
        let d = Ledger {
            generation: 1,
            identity: Some([1; 12]),
            observation: Some(1),
            receipt: Some(1),
        };
        assert_eq!(
            classify(&d, b"marker", FileView::Absent),
            Err(Fault::Integrity)
        );
        for (bytes, identity) in [
            (b"marker".as_slice(), [2; 12]),
            (b"altered".as_slice(), [1; 12]),
        ] {
            assert_eq!(
                classify(&d, b"marker", FileView::Present { bytes, identity }),
                Err(Fault::Integrity)
            );
        }
        assert_eq!(
            classify(&d, b"marker", FileView::Unknown),
            Err(Fault::StorageUnknown)
        );
    }
    #[test]
    fn observation_receipt_and_generation_order_is_not_inferred_from_file_presence() {
        for d in [
            Ledger {
                generation: 0,
                identity: None,
                observation: None,
                receipt: None,
            },
            Ledger {
                generation: 1,
                identity: None,
                observation: Some(1),
                receipt: None,
            },
            Ledger {
                generation: 1,
                identity: Some([1; 12]),
                observation: None,
                receipt: Some(1),
            },
        ] {
            assert_eq!(
                classify(
                    &d,
                    b"m",
                    FileView::Present {
                        bytes: b"m",
                        identity: [1; 12]
                    }
                ),
                Err(Fault::Integrity)
            );
        }
    }
}

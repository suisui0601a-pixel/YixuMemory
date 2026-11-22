//! UTC millisecond bounds preserve source precision, without parsing source text or a calendar.
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    Millisecond,
    Day,
    Month,
    Year,
    Uncertain,
}
impl Precision {
    pub const fn code(self) -> i64 {
        match self {
            Self::Millisecond => 0,
            Self::Day => 1,
            Self::Month => 2,
            Self::Year => 3,
            Self::Uncertain => 4,
        }
    }
    pub fn from_code(n: i64) -> Result<Self> {
        match n {
            0 => Ok(Self::Millisecond),
            1 => Ok(Self::Day),
            2 => Ok(Self::Month),
            3 => Ok(Self::Year),
            4 => Ok(Self::Uncertain),
            _ => Err(Error::Integrity),
        }
    }
}
/// Inclusive possible positions of ONE boundary; precision is retained rather than discarded.
/// Calendar normalization is a future ingestion concern. These are already-normalized UTC bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bound {
    pub earliest_ms: i64,
    pub latest_ms: i64,
    pub precision: Precision,
}
impl Bound {
    pub const fn exact(ms: i64) -> Self {
        Self {
            earliest_ms: ms,
            latest_ms: ms,
            precision: Precision::Millisecond,
        }
    }
    pub fn validate(self) -> Result<()> {
        if self.earliest_ms > self.latest_ms
            || (self.precision == Precision::Millisecond && self.earliest_ms != self.latest_ms)
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    Unknown,
    Known { from: Bound, to: Option<Bound> },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalMatch {
    Certain,
    Possible,
    Unknown,
    Outside,
}
impl Validity {
    pub fn validate(self) -> Result<()> {
        if let Self::Known { from, to } = self {
            from.validate()?;
            if let Some(to) = to {
                to.validate()?;
                if from.latest_ms >= to.earliest_ms {
                    return Err(Error::InvalidInput);
                }
            }
        }
        Ok(())
    }
    /// A known interval is [from,to); unknown is NEVER an unbounded interval.
    pub fn at(self, instant: i64) -> TemporalMatch {
        match self {
            Self::Unknown => TemporalMatch::Unknown,
            Self::Known { from, to } => {
                if instant < from.earliest_ms || to.is_some_and(|b| instant >= b.latest_ms) {
                    TemporalMatch::Outside
                } else if instant >= from.latest_ms && to.is_none_or(|b| instant < b.earliest_ms) {
                    TemporalMatch::Certain
                } else {
                    TemporalMatch::Possible
                }
            }
        }
    }
}

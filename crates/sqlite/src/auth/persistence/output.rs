//! Private W4 experiment. No normal/all-features issuer, plaintext or terminal export.
use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Submitted,
    CancelRequested,
    Stuck,
    Terminal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Count {
    Exact(usize),
    Range(usize, usize),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Accepted,
    Partial,
    Failed,
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
enum PendingState {
    Unsettled,
    NoPending,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    operation: Id,
    connection: Id,
    incarnation: Id,
    attempt: u8,
}
// Constructor is private to this trusted module and its native adapter, never a request DTO.
struct TerminalEvidence {
    identity: Identity,
    count: Count,
    status: u32,
}
struct Operation {
    identity: Identity,
    length: usize,
    phase: Phase,
}
impl Operation {
    fn pending_state(&self) -> PendingState {
        if self.phase == Phase::Terminal {
            PendingState::NoPending
        } else {
            PendingState::Unsettled
        }
    }
    fn new(identity: Identity, length: usize, metadata: usize) -> Result<Self> {
        if identity.attempt != 1 || length == 0 || length > 4096 || metadata > 512 {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            identity,
            length,
            phase: Phase::Prepared,
        })
    }
    fn submit(&mut self) -> Result<()> {
        if self.phase != Phase::Prepared {
            return Err(Error::StateConflict);
        }
        self.phase = Phase::Submitted;
        Ok(())
    }
    fn cancel(&mut self) -> Result<()> {
        if !matches!(
            self.phase,
            Phase::Submitted | Phase::CancelRequested | Phase::Stuck
        ) {
            return Err(Error::StateConflict);
        }
        if self.phase != Phase::Stuck {
            self.phase = Phase::CancelRequested;
        }
        Ok(())
    }
    fn timeout(&mut self) -> Result<()> {
        if !matches!(
            self.phase,
            Phase::Submitted | Phase::CancelRequested | Phase::Stuck
        ) {
            return Err(Error::StateConflict);
        }
        self.phase = Phase::Stuck;
        Ok(())
    }
    fn settle(&mut self, e: TerminalEvidence) -> Result<Outcome> {
        if e.identity != self.identity
            || !matches!(
                self.phase,
                Phase::Submitted | Phase::CancelRequested | Phase::Stuck
            )
        {
            return Err(Error::StateConflict);
        }
        let outcome = match e.count {
            Count::Exact(n) if n <= self.length => {
                if n == self.length {
                    Outcome::Accepted
                } else if n == 0 {
                    Outcome::Failed
                } else {
                    Outcome::Partial
                }
            }
            Count::Range(min, max) if min <= max && max <= self.length => Outcome::Unknown,
            _ => return Err(Error::InvalidInput),
        };
        // Status is diagnostic, never permission to manufacture a terminal observation.
        let _ = e.status;
        self.phase = Phase::Terminal;
        Ok(outcome)
    }
}
mod native;
#[cfg(test)]
mod tests;

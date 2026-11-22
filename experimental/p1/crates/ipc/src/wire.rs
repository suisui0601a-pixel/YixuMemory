//! Canonical framing and a bounded stream decoder.
use crate::Error;

pub const VERSION: u16 = 1;
pub const MAX_PAYLOAD: usize = 512;
pub const HEADER: usize = 52;
pub const MAX_FRAME: usize = HEADER + MAX_PAYLOAD;
pub type Id = [u8; 16];
pub const ZERO: Id = [0; 16];
const MAGIC: [u8; 4] = *b"YXIP";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    Hello = 1,
    Challenge = 2,
    Authenticate = 3,
    Ready = 4,
    Request = 5,
    Cancel = 6,
    Close = 7,
}

impl TryFrom<u8> for Kind {
    type Error = Error;
    fn try_from(value: u8) -> Result<Self, Error> {
        Ok(match value {
            1 => Self::Hello,
            2 => Self::Challenge,
            3 => Self::Authenticate,
            4 => Self::Ready,
            5 => Self::Request,
            6 => Self::Cancel,
            7 => Self::Close,
            _ => return Err(Error::UnknownMessage),
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Frame {
    pub kind: Kind,
    pub session: Id,
    pub request: Id,
    pub seq: u64,
    payload: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("kind", &self.kind)
            .field("seq", &self.seq)
            .field("payload_len", &self.payload.len())
            .finish_non_exhaustive()
    }
}

impl Frame {
    pub fn new(
        kind: Kind,
        session: Id,
        request: Id,
        seq: u64,
        payload: &[u8],
    ) -> Result<Self, Error> {
        if payload.len() > MAX_PAYLOAD {
            return Err(Error::FrameTooLarge);
        }
        Ok(Self {
            kind,
            session,
            request,
            seq,
            payload: payload.to_vec(),
        })
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER + self.payload.len());
        bytes.extend(MAGIC);
        bytes.extend(VERSION.to_be_bytes());
        bytes.extend([self.kind as u8, 0]);
        bytes.extend(self.session);
        bytes.extend(self.request);
        bytes.extend(self.seq.to_be_bytes());
        bytes.extend((self.payload.len() as u32).to_be_bytes());
        bytes.extend(&self.payload);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let size = declared_size(bytes)?;
        if bytes.len() != size {
            return Err(if bytes.len() < size {
                Error::Truncated
            } else {
                Error::TrailingBytes
            });
        }
        Self::new(
            bytes[6].try_into()?,
            field(bytes, 8),
            field(bytes, 24),
            u64::from_be_bytes(field(bytes, 40)),
            &bytes[HEADER..],
        )
    }
}

pub(crate) fn field<const N: usize>(bytes: &[u8], start: usize) -> [u8; N] {
    bytes[start..start + N]
        .try_into()
        .expect("validated record layout")
}

fn declared_size(bytes: &[u8]) -> Result<usize, Error> {
    if bytes.len() < HEADER {
        return Err(Error::Truncated);
    }
    if bytes[..4] != MAGIC {
        return Err(Error::BadEncoding);
    }
    if u16::from_be_bytes(field(bytes, 4)) != VERSION {
        return Err(Error::Version);
    }
    Kind::try_from(bytes[6])?;
    if bytes[7] != 0 {
        return Err(Error::UnknownFlags);
    }
    let n = u32::from_be_bytes(field(bytes, 48));
    if n > MAX_PAYLOAD as u32 {
        return Err(Error::FrameTooLarge);
    }
    Ok(HEADER + n as usize)
}

/// One frame of buffering. The caller retains any unconsumed suffix.
pub struct FrameReader {
    buf: [u8; MAX_FRAME],
    len: usize,
    failed: bool,
}

impl Default for FrameReader {
    fn default() -> Self {
        Self {
            buf: [0; MAX_FRAME],
            len: 0,
            failed: false,
        }
    }
}

impl FrameReader {
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(usize, Option<Frame>), Error> {
        if self.failed {
            return Err(Error::State);
        }
        if chunk.len() > MAX_FRAME {
            self.failed = true;
            return Err(Error::FrameTooLarge);
        }
        let prefix = (HEADER.saturating_sub(self.len)).min(chunk.len());
        self.buf[self.len..self.len + prefix].copy_from_slice(&chunk[..prefix]);
        self.len += prefix;
        if self.len < HEADER {
            return Ok((prefix, None));
        }
        let size = declared_size(&self.buf[..HEADER]).inspect_err(|_| self.failed = true)?;
        let tail = (size - self.len).min(chunk.len() - prefix);
        self.buf[self.len..self.len + tail].copy_from_slice(&chunk[prefix..prefix + tail]);
        self.len += tail;
        let frame = if self.len == size {
            let frame = Frame::decode(&self.buf[..size]).inspect_err(|_| self.failed = true)?;
            self.len = 0;
            Some(frame)
        } else {
            None
        };
        Ok((prefix + tail, frame))
    }

    pub fn finish(self) -> Result<(), Error> {
        if self.failed {
            Err(Error::State)
        } else if self.len != 0 {
            Err(Error::Truncated)
        } else {
            Ok(())
        }
    }
}

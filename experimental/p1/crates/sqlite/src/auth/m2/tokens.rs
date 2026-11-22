//! Test-assembly signing authority. Not Payload encryption, KeyVault or a production key store.
use super::*;
use hmac::{Hmac, KeyInit, Mac};
pub(crate) struct TokenAuthority {
    key: [u8; 32],
}
impl TokenAuthority {
    pub(super) fn random() -> Result<Self> {
        let mut key = [0; 32];
        getrandom::fill(&mut key).map_err(|_| Error::ResourceUnavailable)?;
        Ok(Self { key })
    }
    fn sign(&self, message: &str) -> Result<Vec<u8>> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).map_err(|_| Error::ResourceUnavailable)?;
        mac.update(message.as_bytes());
        let mut out = message.as_bytes().to_vec();
        out.extend_from_slice(&mac.finalize().into_bytes());
        Ok(out)
    }
    fn verify<'a>(&self, token: &'a [u8]) -> Result<&'a str> {
        if token.len() < 33 || token.len() > 2048 {
            return Err(Error::InvalidToken);
        }
        let (message, tag) = token.split_at(token.len() - 32);
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).map_err(|_| Error::ResourceUnavailable)?;
        mac.update(message);
        mac.verify_slice(tag).map_err(|_| Error::InvalidToken)?;
        std::str::from_utf8(message).map_err(|_| Error::InvalidToken)
    }
    pub(super) fn knowledge(&self, ctx: &AuthContext, seq: u64) -> Result<KnowledgeToken> {
        Ok(KnowledgeToken(self.sign(&format!(
            "YXK1|{}|{}|{seq}",
            ctx.realm, ctx.scope.tenant
        ))?))
    }
    pub(super) fn open_knowledge(&self, ctx: &AuthContext, token: &KnowledgeToken) -> Result<u64> {
        let parts: Vec<_> = self.verify(&token.0)?.split('|').collect();
        if parts.len() != 4
            || parts[0] != "YXK1"
            || parts[1] != ctx.realm.to_string()
            || parts[2] != ctx.scope.tenant.to_string()
        {
            return Err(Error::InvalidToken);
        }
        number(parts[3])
    }
    pub(super) fn cursor(&self, fingerprint: &str, seq: u64, offset: usize) -> Result<Cursor> {
        Ok(Cursor(
            self.sign(&format!("YXC1|{fingerprint}|{seq}|{offset}"))?,
        ))
    }
    pub(super) fn open_cursor(&self, fingerprint: &str, cursor: &Cursor) -> Result<(u64, usize)> {
        let parsed = (|| {
            let parts: Vec<_> = self.verify(&cursor.0)?.split('|').collect();
            if parts.len() != 4 || parts[0] != "YXC1" || parts[1] != fingerprint {
                return Err(Error::InvalidToken);
            }
            Ok((
                number(parts[2])?,
                usize::try_from(number(parts[3])?).map_err(|_| Error::InvalidToken)?,
            ))
        })();
        parsed.map_err(|_| Error::InvalidCursor)
    }
}
fn number(s: &str) -> Result<u64> {
    let n = s.parse::<u64>().map_err(|_| Error::InvalidToken)?;
    if n.to_string() != s {
        return Err(Error::InvalidToken);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hmac_sha256_matches_rfc4231_case_one_and_rejects_modified_tag() {
        let mut mac = Hmac::<Sha256>::new_from_slice(&[0x0b; 20]).unwrap();
        mac.update(b"Hi There");
        let tag = mac.finalize().into_bytes();
        let hex = tag.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(
            hex,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        let mut mac = Hmac::<Sha256>::new_from_slice(&[0x0b; 20]).unwrap();
        mac.update(b"Hi There");
        let mut bad = tag;
        bad[0] ^= 1;
        assert!(mac.verify_slice(&bad).is_err());
    }
}

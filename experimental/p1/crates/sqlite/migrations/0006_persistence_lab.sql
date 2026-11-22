
-- Fresh cfg(test) Profile 6 ONLY. Never migrate existing profiles. No production admission.
CREATE TABLE content_intent (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, principal_id TEXT NOT NULL, id TEXT NOT NULL,
 digest BLOB NOT NULL CHECK(length(digest)=32),
 state TEXT NOT NULL CHECK(state IN ('staged','sealed','committed','blocked')),
 event_id TEXT NOT NULL,evidence_id TEXT NOT NULL,memory_id TEXT NOT NULL,revision_id TEXT NOT NULL,
 expected_version INTEGER NOT NULL CHECK(expected_version>=0),
 PRIMARY KEY(tenant_id,principal_id,id), UNIQUE(tenant_id,space_id,revision_id),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TABLE sealed_copy (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,principal_id TEXT NOT NULL,intent_id TEXT NOT NULL,
 payload_id TEXT NOT NULL,owner_id TEXT NOT NULL,owner_kind INTEGER NOT NULL CHECK(owner_kind IN(1,2,3)),
 purpose TEXT NOT NULL CHECK(purpose IN('content','locator')),
 key_id TEXT NOT NULL,seal_generation INTEGER NOT NULL CHECK(seal_generation=1),
 attempted INTEGER NOT NULL CHECK(attempted IN(0,1)),
 envelope_hash BLOB CHECK(envelope_hash IS NULL OR (length(envelope_hash)=32 AND attempted=1)),
 PRIMARY KEY(tenant_id,payload_id),UNIQUE(tenant_id,space_id,payload_id),
 FOREIGN KEY(tenant_id,principal_id,intent_id) REFERENCES content_intent(tenant_id,principal_id,id)
) STRICT;
DROP TRIGGER payload_no_admission;
CREATE TRIGGER payload_no_admission BEFORE INSERT ON payload WHEN
 NEW.state<>'available' OR NEW.effective_classification<>'sensitive' OR NOT EXISTS(
 SELECT 1 FROM sealed_copy s JOIN content_intent i ON i.tenant_id=s.tenant_id AND i.principal_id=s.principal_id AND i.id=s.intent_id
 WHERE s.tenant_id=NEW.tenant_id AND s.space_id=NEW.space_id AND s.payload_id=NEW.id
 AND s.key_id=NEW.key_ref AND NEW.encrypted_ref=s.payload_id AND NEW.seal_generation=s.seal_generation
 AND NEW.purpose=s.purpose AND i.state='sealed' AND s.attempted=1 AND s.envelope_hash IS NOT NULL)
BEGIN SELECT RAISE(ABORT,'sealed receipt required'); END;
DROP TRIGGER memory_lab_only;
CREATE TRIGGER memory_lab_only BEFORE INSERT ON memory WHEN NEW.state<>'active'
BEGIN SELECT RAISE(ABORT,'invalid persistence state'); END;
DROP TRIGGER memory_no_content_transition;
CREATE TRIGGER memory_no_content_transition BEFORE UPDATE OF state ON memory WHEN NEW.state NOT IN('active','source_blocked','deletion_pending')
BEGIN SELECT RAISE(ABORT,'invalid persistence transition'); END;
CREATE TRIGGER sealed_copy_immutable BEFORE UPDATE ON sealed_copy WHEN OLD.envelope_hash IS NOT NULL
BEGIN SELECT RAISE(ABORT,'immutable seal'); END;

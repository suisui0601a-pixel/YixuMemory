-- Fresh-only internal Windows local-content experiment. No migration from Profile 9.
PRAGMA application_id=1498952246;
PRAGMA user_version=10;

CREATE TABLE runtime(
 id INTEGER PRIMARY KEY CHECK(id=1),
 installation BLOB NOT NULL CHECK(length(installation)=16),
 realm BLOB NOT NULL CHECK(length(realm)=16),
 incarnation BLOB NOT NULL CHECK(length(incarnation)=16),
 owner_nonce BLOB NOT NULL CHECK(length(owner_nonce)=16),
 store_identity BLOB NOT NULL CHECK(length(store_identity)=36),
 schema_hash BLOB NOT NULL CHECK(length(schema_hash)=32),
 epoch INTEGER NOT NULL CHECK(epoch>0),
 knowledge_seq INTEGER NOT NULL CHECK(knowledge_seq>=0),
 blocked INTEGER NOT NULL DEFAULT 0 CHECK(blocked IN (0,1)),
 clean INTEGER NOT NULL DEFAULT 0 CHECK(clean IN (0,1))
) STRICT;
CREATE TRIGGER runtime_identity BEFORE UPDATE ON runtime
WHEN NEW.id<>OLD.id OR NEW.installation<>OLD.installation OR NEW.realm<>OLD.realm
 OR NEW.schema_hash<>OLD.schema_hash OR NEW.store_identity<>OLD.store_identity OR NEW.epoch<OLD.epoch
 OR NEW.knowledge_seq<OLD.knowledge_seq OR NEW.blocked<OLD.blocked
BEGIN SELECT RAISE(ABORT,'runtime identity or monotonicity'); END;
CREATE TABLE stop_receipt(
 incarnation BLOB PRIMARY KEY CHECK(length(incarnation)=16),
 installation BLOB NOT NULL,
 realm BLOB NOT NULL,
 owner_nonce BLOB NOT NULL,
 epoch INTEGER NOT NULL,
 knowledge_seq INTEGER NOT NULL,
 schema_hash BLOB NOT NULL,
 ledger_hash BLOB NOT NULL CHECK(length(ledger_hash)=32)
) STRICT;
CREATE TABLE resume_receipt(
 previous_incarnation BLOB PRIMARY KEY REFERENCES stop_receipt(incarnation),
 next_incarnation BLOB NOT NULL UNIQUE CHECK(length(next_incarnation)=16),
 epoch INTEGER NOT NULL,
 ledger_hash BLOB NOT NULL CHECK(length(ledger_hash)=32)
) STRICT;

-- Same R4 authorization projection, evaluated by its shared transaction gate.
CREATE TABLE authority(
 scope BLOB NOT NULL CHECK(length(scope)=48),
 principal BLOB NOT NULL CHECK(length(principal)=16),
 target BLOB NOT NULL CHECK(length(target)=16),
 epoch INTEGER NOT NULL CHECK(epoch>0),
 expiry INTEGER NOT NULL CHECK(expiry>0),
 grant_ok INTEGER NOT NULL DEFAULT 1 CHECK(grant_ok IN (0,1)),
 principal_ok INTEGER NOT NULL DEFAULT 1 CHECK(principal_ok IN (0,1)),
 space_ok INTEGER NOT NULL DEFAULT 1 CHECK(space_ok IN (0,1)),
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
 source INTEGER NOT NULL DEFAULT 1 CHECK(source>0),
 key_gen INTEGER NOT NULL DEFAULT 1 CHECK(key_gen>0),
 copy_gen INTEGER NOT NULL DEFAULT 1 CHECK(copy_gen>0),
 deletion_gen INTEGER NOT NULL DEFAULT 0 CHECK(deletion_gen>=0),
 source_ok INTEGER NOT NULL DEFAULT 1 CHECK(source_ok IN (0,1)),
 key_ok INTEGER NOT NULL DEFAULT 1 CHECK(key_ok IN (0,1)),
 copy_ok INTEGER NOT NULL DEFAULT 1 CHECK(copy_ok IN (0,1)),
 PRIMARY KEY(scope,principal,target)
) STRICT;
CREATE TABLE source(
 scope BLOB NOT NULL,
 id BLOB NOT NULL CHECK(length(id)=16),
 generation INTEGER NOT NULL CHECK(generation>0),
 valid INTEGER NOT NULL CHECK(valid IN (0,1)),
 PRIMARY KEY(scope,id,generation)
) STRICT;
CREATE TABLE evidence(
 scope BLOB NOT NULL,
 id BLOB NOT NULL CHECK(length(id)=16),
 generation INTEGER NOT NULL CHECK(generation>0),
 source BLOB NOT NULL,
 source_generation INTEGER NOT NULL,
 valid INTEGER NOT NULL CHECK(valid IN (0,1)),
 PRIMARY KEY(scope,id,generation),
 FOREIGN KEY(scope,source,source_generation) REFERENCES source(scope,id,generation)
) STRICT;
CREATE TABLE security_event(
 id INTEGER PRIMARY KEY,
 scope BLOB NOT NULL CHECK(length(scope)=48),
 kind TEXT NOT NULL CHECK(kind IN ('Write','Revocation')),
 state TEXT NOT NULL DEFAULT 'Closing' CHECK(state IN ('Closing','EffectiveFence','Applied')),
 UNIQUE(id,state)
) STRICT;
CREATE TABLE security_receipt(
 event INTEGER PRIMARY KEY,
 epoch INTEGER NOT NULL,
 phase TEXT NOT NULL DEFAULT 'Applied' CHECK(phase='Applied'),
 FOREIGN KEY(event,phase) REFERENCES security_event(id,state) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r5_tombstone(
 scope BLOB NOT NULL CHECK(length(scope)=48),
 object BLOB NOT NULL CHECK(length(object)=16),
 generation INTEGER NOT NULL CHECK(generation>0),
 event INTEGER NOT NULL REFERENCES security_receipt(event) DEFERRABLE INITIALLY DEFERRED,
 PRIMARY KEY(scope,object,generation)
) STRICT;
CREATE TRIGGER tombstone_fenced BEFORE INSERT ON r5_tombstone
WHEN NOT EXISTS(SELECT 1 FROM security_event WHERE id=NEW.event
 AND scope=NEW.scope AND state='EffectiveFence')
BEGIN SELECT RAISE(ABORT,'retirement requires effective fence'); END;
CREATE TABLE write_intent(
 operation BLOB PRIMARY KEY CHECK(length(operation)=16),
 scope BLOB NOT NULL,
 principal BLOB NOT NULL,
 request BLOB NOT NULL CHECK(length(request)=16 AND request<>zeroblob(16)),
 memory BLOB NOT NULL,
 revision BLOB NOT NULL CHECK(length(revision)=16 AND revision<>zeroblob(16)),
 payload BLOB NOT NULL UNIQUE CHECK(length(payload)=16),
 key_id BLOB NOT NULL UNIQUE CHECK(length(key_id)=16),
 evidence BLOB NOT NULL,
 evidence_generation INTEGER NOT NULL CHECK(evidence_generation>0),
 generation INTEGER NOT NULL CHECK(generation=1),
 expected_current BLOB CHECK(expected_current IS NULL OR length(expected_current)=16),
 incarnation BLOB NOT NULL CHECK(length(incarnation)=16),
 epoch INTEGER NOT NULL,
 dispatch_binding BLOB NOT NULL CHECK(length(dispatch_binding)=32),
 state TEXT NOT NULL CHECK(state IN ('Pending','Staged','Sealing','Published','Committed','Unknown')),
 UNIQUE(scope,principal,request),
 UNIQUE(operation,scope,memory,revision,payload,key_id,generation),
 FOREIGN KEY(scope,principal,memory) REFERENCES authority(scope,principal,target),
 FOREIGN KEY(scope,evidence,evidence_generation) REFERENCES evidence(scope,id,generation)
) STRICT;
CREATE TABLE publication(
 operation BLOB PRIMARY KEY REFERENCES write_intent(operation),
 cipher_hash BLOB NOT NULL CHECK(length(cipher_hash)=32),
 wrapper_hash BLOB NOT NULL CHECK(length(wrapper_hash)=32),
 cipher_identity BLOB NOT NULL CHECK(length(cipher_identity)=12),
 wrapper_identity BLOB NOT NULL CHECK(length(wrapper_identity)=12),
 binding BLOB NOT NULL
) STRICT;
CREATE UNIQUE INDEX one_unfinished_write ON write_intent((1)) WHERE state<>'Committed';
CREATE TABLE memory(
 scope BLOB NOT NULL,
 id BLOB NOT NULL,
 current_revision BLOB NOT NULL,
 state TEXT NOT NULL CHECK(state='active'),
 PRIMARY KEY(scope,id),
 FOREIGN KEY(scope,id,current_revision) REFERENCES revision(scope,memory,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE revision(
 scope BLOB NOT NULL,
 memory BLOB NOT NULL,
 id BLOB NOT NULL,
 number INTEGER NOT NULL CHECK(number>0),
 payload BLOB NOT NULL,
 PRIMARY KEY(scope,memory,id),
 UNIQUE(scope,memory,number),
 UNIQUE(scope,memory,id,payload),
 FOREIGN KEY(scope,memory) REFERENCES memory(scope,id) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(scope,memory,id,payload) REFERENCES payload(scope,memory,revision,id) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(scope,memory,id) REFERENCES revision_evidence(scope,memory,revision) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE payload(
 scope BLOB NOT NULL,
 memory BLOB NOT NULL,
 revision BLOB NOT NULL,
 id BLOB NOT NULL,
 key_id BLOB NOT NULL,
 generation INTEGER NOT NULL,
 operation BLOB NOT NULL UNIQUE REFERENCES publication(operation),
 PRIMARY KEY(scope,memory,revision,id),
 FOREIGN KEY(operation,scope,memory,revision,id,key_id,generation)
   REFERENCES write_intent(operation,scope,memory,revision,payload,key_id,generation),
 FOREIGN KEY(scope,id,key_id,generation) REFERENCES local_key(scope,payload,id,generation) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE local_key(
 scope BLOB NOT NULL,
 payload BLOB NOT NULL,
 id BLOB NOT NULL,
 generation INTEGER NOT NULL,
 wrap_version INTEGER NOT NULL CHECK(wrap_version=1),
 revoked INTEGER NOT NULL CHECK(revoked IN (0,1)),
 PRIMARY KEY(scope,payload,id,generation)
) STRICT;
CREATE TABLE revision_evidence(
 scope BLOB NOT NULL,
 memory BLOB NOT NULL,
 revision BLOB NOT NULL,
 evidence BLOB NOT NULL,
 generation INTEGER NOT NULL,
 PRIMARY KEY(scope,memory,revision),
 FOREIGN KEY(scope,memory,revision) REFERENCES revision(scope,memory,id) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(scope,evidence,generation) REFERENCES evidence(scope,id,generation)
) STRICT;
CREATE TABLE receipt(
 operation BLOB PRIMARY KEY REFERENCES write_intent(operation),
 scope BLOB NOT NULL,
 memory BLOB NOT NULL,
 revision BLOB NOT NULL,
 knowledge_seq INTEGER NOT NULL UNIQUE CHECK(knowledge_seq>0),
 event INTEGER NOT NULL REFERENCES security_receipt(event) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(scope,memory,revision) REFERENCES revision(scope,memory,id)
) STRICT;
CREATE TABLE outbox(
 operation BLOB PRIMARY KEY REFERENCES receipt(operation),
 action TEXT NOT NULL CHECK(action='MemoryCreated'),
 knowledge_seq INTEGER NOT NULL UNIQUE REFERENCES receipt(knowledge_seq)
) STRICT;
CREATE TABLE transition(
 operation BLOB NOT NULL REFERENCES write_intent(operation),
 phase TEXT NOT NULL CHECK(phase IN ('Pending','Staged','Sealing','Published','Committed','Unknown')),
 PRIMARY KEY(operation,phase)
) STRICT;
CREATE TRIGGER intent_insert BEFORE INSERT ON write_intent
WHEN NEW.state<>'Pending' OR (SELECT blocked FROM runtime WHERE id=1)<>0
 OR (SELECT clean FROM runtime WHERE id=1)<>0
 OR EXISTS(SELECT 1 FROM security_event WHERE state<>'Applied')
BEGIN SELECT RAISE(ABORT,'write admission closed'); END;
CREATE TRIGGER intent_progress BEFORE UPDATE ON write_intent
WHEN NEW.operation<>OLD.operation OR NEW.scope<>OLD.scope OR NEW.principal<>OLD.principal
 OR NEW.request<>OLD.request OR NEW.memory<>OLD.memory OR NEW.revision<>OLD.revision
 OR NEW.payload<>OLD.payload OR NEW.key_id<>OLD.key_id OR NEW.evidence<>OLD.evidence
 OR NEW.evidence_generation<>OLD.evidence_generation OR NEW.generation<>OLD.generation
 OR NEW.expected_current IS NOT OLD.expected_current OR NEW.incarnation<>OLD.incarnation
 OR NEW.epoch<>OLD.epoch OR NEW.dispatch_binding<>OLD.dispatch_binding
 OR NOT ((OLD.state='Pending' AND NEW.state='Staged')
 OR (OLD.state='Staged' AND NEW.state='Sealing')
 OR (OLD.state='Sealing' AND NEW.state='Published')
 OR (OLD.state='Published' AND NEW.state='Committed')
 OR (OLD.state IN ('Pending','Staged','Sealing','Published') AND NEW.state='Unknown'))
BEGIN SELECT RAISE(ABORT,'immutable request or invalid transition'); END;
CREATE TRIGGER intent_created AFTER INSERT ON write_intent
BEGIN INSERT INTO transition VALUES(NEW.operation,NEW.state); END;
CREATE TRIGGER intent_changed AFTER UPDATE ON write_intent
BEGIN INSERT INTO transition VALUES(NEW.operation,NEW.state); END;
CREATE TRIGGER security_progress BEFORE UPDATE ON security_event
WHEN NEW.id<>OLD.id OR NEW.scope<>OLD.scope OR NEW.kind<>OLD.kind
 OR NOT ((OLD.state='Closing' AND NEW.state='EffectiveFence') OR (OLD.state='EffectiveFence' AND NEW.state='Applied'))
 OR EXISTS(SELECT 1 FROM security_event WHERE id<OLD.id AND state<>'Applied')
 OR (SELECT blocked FROM runtime WHERE id=1)<>0
 OR (NEW.kind='Revocation' AND NEW.state='EffectiveFence'
     AND EXISTS(SELECT 1 FROM write_intent WHERE state<>'Committed'))
 OR (NEW.state='Applied' AND NOT EXISTS(SELECT 1 FROM security_receipt WHERE event=NEW.id))
BEGIN SELECT RAISE(ABORT,'security ordering'); END;
CREATE TRIGGER security_initial BEFORE INSERT ON security_event
WHEN NEW.state<>'Closing' OR (SELECT blocked FROM runtime)<>0 OR (SELECT clean FROM runtime)<>0
BEGIN SELECT RAISE(ABORT,'security event requires live owner'); END;
CREATE TRIGGER seal_requires_receipt BEFORE UPDATE OF state ON write_intent
WHEN NEW.state='Published' AND NOT EXISTS(SELECT 1 FROM publication WHERE operation=NEW.operation)
BEGIN SELECT RAISE(ABORT,'publication unconfirmed'); END;
CREATE TRIGGER commit_requires_graph BEFORE UPDATE OF state ON write_intent
WHEN NEW.state='Committed' AND NOT EXISTS(
 SELECT 1 FROM receipt r JOIN outbox o USING(operation) JOIN revision v
 ON v.scope=r.scope AND v.memory=r.memory AND v.id=r.revision
 WHERE r.operation=NEW.operation AND o.knowledge_seq=r.knowledge_seq)
BEGIN SELECT RAISE(ABORT,'commit requires atomic graph and receipt'); END;
CREATE TRIGGER revision_number BEFORE INSERT ON revision
WHEN NEW.number<>coalesce((SELECT max(number)+1 FROM revision WHERE scope=NEW.scope AND memory=NEW.memory),1)
BEGIN SELECT RAISE(ABORT,'revision sequence'); END;
CREATE TRIGGER memory_current BEFORE UPDATE ON memory
WHEN NEW.scope<>OLD.scope OR NEW.id<>OLD.id OR NOT EXISTS(
 SELECT 1 FROM revision n JOIN revision p ON n.scope=p.scope AND n.memory=p.memory
 WHERE n.id=NEW.current_revision AND p.id=OLD.current_revision
 AND n.scope=OLD.scope AND n.memory=OLD.id AND n.number>p.number)
BEGIN SELECT RAISE(ABORT,'current revision forward only'); END;
CREATE TRIGGER publication_phase BEFORE INSERT ON publication
WHEN (SELECT state FROM write_intent WHERE operation=NEW.operation) IS NOT 'Sealing'
BEGIN SELECT RAISE(ABORT,'publication phase'); END;
CREATE TRIGGER authority_ordered BEFORE UPDATE ON authority
WHEN NOT EXISTS(SELECT 1 FROM security_event WHERE state='EffectiveFence')
BEGIN SELECT RAISE(ABORT,'authority requires effective fence'); END;
CREATE TRIGGER receipt_atomic BEFORE INSERT ON receipt
WHEN NOT EXISTS(SELECT 1 FROM write_intent i JOIN security_event e ON e.id=NEW.event
 JOIN security_receipt s ON s.event=e.id JOIN payload p ON p.operation=i.operation
 WHERE i.operation=NEW.operation AND i.state='Published' AND i.scope=NEW.scope
 AND i.memory=NEW.memory AND i.revision=NEW.revision AND e.kind='Write'
 AND e.state='Applied' AND e.scope=NEW.scope
 AND s.epoch=(SELECT epoch FROM runtime)
 AND NEW.knowledge_seq=(SELECT knowledge_seq FROM runtime))
BEGIN SELECT RAISE(ABORT,'receipt requires atomic security and business commit'); END;
CREATE TRIGGER security_receipt_fenced BEFORE INSERT ON security_receipt
WHEN (SELECT state FROM security_event WHERE id=NEW.event) IS NOT 'EffectiveFence'
 OR NEW.epoch<>(SELECT epoch FROM runtime)
BEGIN SELECT RAISE(ABORT,'security receipt without effective fence'); END;
CREATE TRIGGER revision_fenced BEFORE INSERT ON revision
WHEN NOT EXISTS(SELECT 1 FROM security_event WHERE scope=NEW.scope AND kind='Write' AND state='EffectiveFence')
BEGIN SELECT RAISE(ABORT,'business write requires effective fence'); END;

CREATE TRIGGER immutable_revision_UPDATE BEFORE UPDATE ON revision
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_revision_DELETE BEFORE DELETE ON revision
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_payload_UPDATE BEFORE UPDATE ON payload
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_payload_DELETE BEFORE DELETE ON payload
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_local_key_UPDATE BEFORE UPDATE ON local_key
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_local_key_DELETE BEFORE DELETE ON local_key
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_revision_evidence_UPDATE BEFORE UPDATE ON revision_evidence
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_revision_evidence_DELETE BEFORE DELETE ON revision_evidence
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_publication_UPDATE BEFORE UPDATE ON publication
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_publication_DELETE BEFORE DELETE ON publication
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_receipt_UPDATE BEFORE UPDATE ON receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_receipt_DELETE BEFORE DELETE ON receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_outbox_UPDATE BEFORE UPDATE ON outbox
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_outbox_DELETE BEFORE DELETE ON outbox
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_transition_UPDATE BEFORE UPDATE ON transition
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_transition_DELETE BEFORE DELETE ON transition
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_security_receipt_UPDATE BEFORE UPDATE ON security_receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_security_receipt_DELETE BEFORE DELETE ON security_receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_r5_tombstone_UPDATE BEFORE UPDATE ON r5_tombstone
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_r5_tombstone_DELETE BEFORE DELETE ON r5_tombstone
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_stop_receipt_UPDATE BEFORE UPDATE ON stop_receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_stop_receipt_DELETE BEFORE DELETE ON stop_receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_resume_receipt_UPDATE BEFORE UPDATE ON resume_receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER immutable_resume_receipt_DELETE BEFORE DELETE ON resume_receipt
BEGIN SELECT RAISE(ABORT,'immutable ledger'); END;

CREATE TRIGGER keep_runtime BEFORE DELETE ON runtime
BEGIN SELECT RAISE(ABORT,'retained history'); END;

CREATE TRIGGER keep_authority BEFORE DELETE ON authority
BEGIN SELECT RAISE(ABORT,'retained history'); END;

CREATE TRIGGER keep_source BEFORE DELETE ON source
BEGIN SELECT RAISE(ABORT,'retained history'); END;

CREATE TRIGGER keep_evidence BEFORE DELETE ON evidence
BEGIN SELECT RAISE(ABORT,'retained history'); END;

CREATE TRIGGER keep_memory BEFORE DELETE ON memory
BEGIN SELECT RAISE(ABORT,'retained history'); END;

CREATE TRIGGER keep_write_intent BEFORE DELETE ON write_intent
BEGIN SELECT RAISE(ABORT,'retained history'); END;

CREATE TRIGGER keep_security_event BEFORE DELETE ON security_event
BEGIN SELECT RAISE(ABORT,'retained history'); END;

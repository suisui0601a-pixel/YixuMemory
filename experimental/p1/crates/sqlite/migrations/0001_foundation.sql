-- PRIVATE metadata lab. No plaintext, ciphertext, keys, JSON or free-form content columns.
CREATE TABLE tenant (
 id TEXT PRIMARY KEY NOT NULL CHECK(length(id)=36),
 status TEXT NOT NULL CHECK(status IN ('active','disabled')),
 auth_epoch INTEGER NOT NULL CHECK(auth_epoch>0),
 knowledge_seq INTEGER NOT NULL CHECK(knowledge_seq>=0)
) STRICT;
CREATE TABLE principal (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 kind TEXT NOT NULL CHECK(kind IN ('user','service')),
 status TEXT NOT NULL CHECK(status IN ('active','disabled')),
 PRIMARY KEY(tenant_id,id), FOREIGN KEY(tenant_id) REFERENCES tenant(id)
) STRICT;
CREATE TABLE principal_group (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 kind TEXT NOT NULL CHECK(kind IN ('role','team','project')),
 PRIMARY KEY(tenant_id,id), FOREIGN KEY(tenant_id) REFERENCES tenant(id)
) STRICT;
CREATE TABLE space (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 scope TEXT NOT NULL CHECK(scope IN ('personal','role','team','project')),
 owner_principal_id TEXT, owner_group_id TEXT,
 policy_epoch INTEGER NOT NULL CHECK(policy_epoch>0),
 graph_version INTEGER NOT NULL CHECK(graph_version>=0),
 security_floor TEXT NOT NULL CHECK(security_floor IN ('personal','sensitive')),
 PRIMARY KEY(tenant_id,id), FOREIGN KEY(tenant_id) REFERENCES tenant(id),
 FOREIGN KEY(tenant_id,owner_principal_id) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,owner_group_id) REFERENCES principal_group(tenant_id,id),
 CHECK((scope='personal' AND owner_principal_id IS NOT NULL AND owner_group_id IS NULL)
    OR (scope<>'personal' AND owner_principal_id IS NULL AND owner_group_id IS NOT NULL))
) STRICT;
CREATE TABLE commit_record (
 tenant_id TEXT NOT NULL, knowledge_seq INTEGER NOT NULL CHECK(knowledge_seq>0),
 accepted_at_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,knowledge_seq), FOREIGN KEY(tenant_id) REFERENCES tenant(id)
) STRICT;
CREATE TABLE payload (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 purpose TEXT NOT NULL CHECK(purpose IN ('content','locator','provenance','index_projection')),
 declared_classification TEXT CHECK(declared_classification IN ('synthetic','personal','sensitive')),
 effective_classification TEXT NOT NULL CHECK(effective_classification IN ('personal','sensitive')),
 protection_policy_epoch INTEGER NOT NULL CHECK(protection_policy_epoch>0),
 encrypted_ref TEXT CHECK(encrypted_ref IS NULL OR length(encrypted_ref)=36),
 key_ref TEXT CHECK(key_ref IS NULL OR length(key_ref)=36),
 encryption_profile TEXT NOT NULL CHECK(encryption_profile='metadata-fixture-not-encrypted'),
 seal_context_id TEXT NOT NULL CHECK(length(seal_context_id)=36),
 seal_generation INTEGER NOT NULL CHECK(seal_generation>0),
 state TEXT NOT NULL CHECK(state IN ('staged','available','erasure_pending','blocked','erased')),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,id),
 UNIQUE(key_ref), UNIQUE(encrypted_ref),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 CHECK((state='erased' AND encrypted_ref IS NULL AND key_ref IS NULL)
 OR (state<>'erased' AND encrypted_ref IS NOT NULL AND key_ref IS NOT NULL))
) STRICT;
CREATE TABLE event (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 kind TEXT NOT NULL CHECK(kind IN ('raw_event','external_document','user_statement','system_action')),
 payload_id TEXT, actor_id TEXT NOT NULL,
 occurred_at_ms INTEGER NOT NULL, recorded_at_ms INTEGER NOT NULL,
 knowledge_seq INTEGER NOT NULL, schema_version TEXT NOT NULL CHECK(schema_version='1.0'),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,actor_id) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,payload_id) REFERENCES payload(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq)
) STRICT;
CREATE TABLE evidence (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 event_id TEXT NOT NULL, payload_id TEXT NOT NULL, locator_payload_id TEXT NOT NULL,
 source_kind TEXT NOT NULL CHECK(source_kind IN ('raw_event','external_document','user_statement','model_output','fictional_setting')),
 captured_at_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,payload_id) REFERENCES payload(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,locator_payload_id) REFERENCES payload(tenant_id,space_id,id)
) STRICT;
CREATE TABLE memory (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 current_revision_id TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version>0),
 state TEXT NOT NULL CHECK(state IN ('active','archived','source_blocked','deletion_pending','purged')),
 created_at_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,id,current_revision_id)
 REFERENCES revision(tenant_id,space_id,memory_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE revision (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 memory_id TEXT NOT NULL, number INTEGER NOT NULL CHECK(number>0),
 parent_revision_id TEXT, parent_number INTEGER,
 primary_evidence_id TEXT NOT NULL,
 content_payload_id TEXT NOT NULL,
 assertion_kind TEXT NOT NULL CHECK(assertion_kind IN ('extracted_fact','model_inference','fictional_setting','user_assertion')),
 valid_from_ms INTEGER NOT NULL, valid_to_ms INTEGER,
 recorded_at_ms INTEGER NOT NULL, knowledge_seq INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,id),
 UNIQUE(tenant_id,space_id,memory_id,id),
 UNIQUE(tenant_id,space_id,memory_id,number),
 UNIQUE(tenant_id,space_id,memory_id,id,number),
 FOREIGN KEY(tenant_id,space_id,memory_id) REFERENCES memory(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,memory_id,parent_revision_id,parent_number)
 REFERENCES revision(tenant_id,space_id,memory_id,id,number) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant_id,space_id,id,primary_evidence_id)
 REFERENCES revision_evidence(tenant_id,space_id,revision_id,evidence_id) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant_id,space_id,content_payload_id) REFERENCES payload(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq),
 CHECK((number=1 AND parent_revision_id IS NULL AND parent_number IS NULL)
    OR (number>1 AND parent_revision_id IS NOT NULL AND parent_number IS NOT NULL AND parent_number=number-1)),
 CHECK(valid_to_ms IS NULL OR valid_to_ms>valid_from_ms)
) STRICT;
CREATE TABLE revision_evidence (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, revision_id TEXT NOT NULL, evidence_id TEXT NOT NULL,
 relation TEXT NOT NULL CHECK(relation IN ('supports','contradicts','context')),
 PRIMARY KEY(tenant_id,space_id,revision_id,evidence_id),
 FOREIGN KEY(tenant_id,space_id,revision_id) REFERENCES revision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,evidence_id) REFERENCES evidence(tenant_id,space_id,id)
) STRICT;
CREATE TABLE idempotency_record (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 actor_id TEXT NOT NULL, space_id TEXT NOT NULL,
 operation TEXT NOT NULL CHECK(operation IN ('metadata_append','metadata_revise')),
 key_lookup BLOB NOT NULL CHECK(length(key_lookup)=32),
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 result_memory_id TEXT NOT NULL, expires_at_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,actor_id,operation,key_lookup),
 FOREIGN KEY(tenant_id,actor_id) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,result_memory_id) REFERENCES memory(tenant_id,space_id,id)
) STRICT;
CREATE TABLE deletion_request (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 operation TEXT NOT NULL CHECK(operation IN ('memory_graph','evidence_graph','source_graph')),
 requested_scope TEXT NOT NULL CHECK(requested_scope='space'),
 effective_scope TEXT NOT NULL CHECK(effective_scope='space'),
 target_type TEXT NOT NULL, target_id TEXT NOT NULL,
 memory_id TEXT, evidence_id TEXT, event_id TEXT,
 status TEXT NOT NULL CHECK(status IN ('queued','running','verifying','blocked','completed')),
 graph_cut_off INTEGER NOT NULL CHECK(graph_cut_off>0),
 closure_ref TEXT NOT NULL, closure_version INTEGER NOT NULL CHECK(closure_version>0),
 policy_epoch INTEGER NOT NULL CHECK(policy_epoch>0),
 deletion_generation INTEGER NOT NULL CHECK(deletion_generation>0),
 requested_by TEXT NOT NULL, coverage TEXT NOT NULL CHECK(coverage IN ('pending','space_graph_complete')),
 receipt_set_ref TEXT,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,id),
 UNIQUE(tenant_id,space_id,id,deletion_generation,closure_version),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,requested_by) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,memory_id) REFERENCES memory(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,evidence_id) REFERENCES evidence(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,id,closure_ref,closure_version)
 REFERENCES deletion_closure(tenant_id,space_id,deletion_id,id,version) DEFERRABLE INITIALLY DEFERRED,
 CHECK((operation='memory_graph' AND target_type='memory' AND memory_id=target_id AND memory_id IS NOT NULL AND evidence_id IS NULL AND event_id IS NULL)
 OR (operation='evidence_graph' AND target_type='evidence' AND evidence_id=target_id AND evidence_id IS NOT NULL AND memory_id IS NULL AND event_id IS NULL)
 OR (operation='source_graph' AND target_type='event' AND event_id=target_id AND event_id IS NOT NULL AND memory_id IS NULL AND evidence_id IS NULL)),
 CHECK((status='completed' AND coverage='space_graph_complete' AND receipt_set_ref IS NOT NULL)
 OR (status<>'completed' AND coverage='pending' AND receipt_set_ref IS NULL))
) STRICT;
CREATE TABLE deletion_closure (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, deletion_id TEXT NOT NULL, id TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version>0),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,deletion_id,id,version),
 FOREIGN KEY(tenant_id,space_id,deletion_id) REFERENCES deletion_request(tenant_id,space_id,id)
) STRICT;
CREATE TABLE erasure_receipt (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, space_id TEXT NOT NULL, deletion_id TEXT NOT NULL,
 store TEXT NOT NULL CHECK(store IN ('payload','key_backend','index','cache','export','staging','backup','jobs','metadata_mappings','temporary')),
 copy_id TEXT NOT NULL, seal_generation INTEGER NOT NULL CHECK(seal_generation>0),
 deletion_generation INTEGER NOT NULL CHECK(deletion_generation>0),
 closure_version INTEGER NOT NULL CHECK(closure_version>0),
 status TEXT NOT NULL CHECK(status IN ('pending','failed','verified')),
 completed_at_ms INTEGER,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,deletion_id,store,copy_id,seal_generation,deletion_generation,closure_version),
 FOREIGN KEY(tenant_id,space_id,deletion_id,deletion_generation,closure_version)
 REFERENCES deletion_request(tenant_id,space_id,id,deletion_generation,closure_version),
 FOREIGN KEY(tenant_id,space_id,copy_id,seal_generation,store)
 REFERENCES copy_manifest(tenant_id,space_id,id,seal_generation,store)
) STRICT;
CREATE TABLE copy_manifest (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 payload_id TEXT, seal_generation INTEGER NOT NULL CHECK(seal_generation>0),
 store TEXT NOT NULL CHECK(store IN ('payload','key_backend','index','cache','export','staging','backup','jobs','metadata_mappings','temporary')),
 state TEXT NOT NULL CHECK(state IN ('registered','erasure_pending','unknown')),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,space_id,id,seal_generation,store),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,payload_id) REFERENCES payload(tenant_id,space_id,id)
) STRICT;
CREATE TABLE revision_invalidation (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36), space_id TEXT NOT NULL,
 revision_id TEXT NOT NULL, deletion_id TEXT NOT NULL,
 deletion_generation INTEGER NOT NULL CHECK(deletion_generation>0),
 closure_version INTEGER NOT NULL CHECK(closure_version>0),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,revision_id,deletion_id,deletion_generation),
 FOREIGN KEY(tenant_id,space_id,revision_id) REFERENCES revision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,deletion_id,deletion_generation,closure_version)
 REFERENCES deletion_request(tenant_id,space_id,id,deletion_generation,closure_version)
) STRICT;
CREATE TABLE outbox_job (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 kind TEXT NOT NULL CHECK(kind IN ('metadata_changed','erase')),
 aggregate_id TEXT NOT NULL, memory_id TEXT, deletion_id TEXT,
 generation INTEGER NOT NULL CHECK(generation>0),
 status TEXT NOT NULL CHECK(status IN ('pending','leased','succeeded','dead')),
 lease_until_ms INTEGER, lease_token INTEGER NOT NULL CHECK(lease_token>=0),
 attempts INTEGER NOT NULL CHECK(attempts>=0), next_run_at_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,kind,aggregate_id,generation),
 FOREIGN KEY(tenant_id,space_id,memory_id) REFERENCES memory(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,deletion_id) REFERENCES deletion_request(tenant_id,space_id,id),
 CHECK((kind='metadata_changed' AND memory_id=aggregate_id AND memory_id IS NOT NULL AND deletion_id IS NULL)
 OR (kind='erase' AND deletion_id=aggregate_id AND deletion_id IS NOT NULL AND memory_id IS NULL))
) STRICT;
-- No verified cryptography/coordinator is implemented. SQL flags cannot enable content or completion.
CREATE TRIGGER payload_no_admission BEFORE INSERT ON payload WHEN NEW.state<>'blocked'
BEGIN SELECT RAISE(ABORT,'content capability unavailable'); END;
CREATE TRIGGER payload_no_state_change BEFORE UPDATE ON payload
BEGIN SELECT RAISE(ABORT,'content capability unavailable'); END;
CREATE TRIGGER evidence_payload_roles BEFORE INSERT ON evidence
WHEN EXISTS(SELECT 1 FROM payload p WHERE p.tenant_id=NEW.tenant_id AND p.id=NEW.payload_id AND p.purpose<>'content')
 OR EXISTS(SELECT 1 FROM payload p WHERE p.tenant_id=NEW.tenant_id AND p.id=NEW.locator_payload_id AND p.purpose<>'locator')
BEGIN SELECT RAISE(ABORT,'payload role mismatch'); END;
CREATE TRIGGER revision_private_payload BEFORE INSERT ON revision
WHEN EXISTS(SELECT 1 FROM payload p WHERE p.tenant_id=NEW.tenant_id AND p.id=NEW.content_payload_id AND p.purpose<>'content')
 OR EXISTS(SELECT 1 FROM revision r WHERE r.tenant_id=NEW.tenant_id AND r.content_payload_id=NEW.content_payload_id)
 OR EXISTS(SELECT 1 FROM event e WHERE e.tenant_id=NEW.tenant_id AND e.payload_id=NEW.content_payload_id)
 OR EXISTS(SELECT 1 FROM evidence e WHERE e.tenant_id=NEW.tenant_id AND (e.payload_id=NEW.content_payload_id OR e.locator_payload_id=NEW.content_payload_id))
BEGIN SELECT RAISE(ABORT,'revision content must be an independent erasure unit'); END;
CREATE TRIGGER event_no_revision_payload BEFORE INSERT ON event
WHEN EXISTS(SELECT 1 FROM revision r WHERE r.tenant_id=NEW.tenant_id AND r.content_payload_id=NEW.payload_id)
BEGIN SELECT RAISE(ABORT,'revision content must be an independent erasure unit'); END;
CREATE TRIGGER evidence_no_revision_payload BEFORE INSERT ON evidence
WHEN EXISTS(SELECT 1 FROM revision r WHERE r.tenant_id=NEW.tenant_id AND (r.content_payload_id=NEW.payload_id OR r.content_payload_id=NEW.locator_payload_id))
BEGIN SELECT RAISE(ABORT,'revision content must be an independent erasure unit'); END;
CREATE TRIGGER memory_lab_only BEFORE INSERT ON memory WHEN NEW.state<>'source_blocked'
BEGIN SELECT RAISE(ABORT,'content capability unavailable'); END;
CREATE TRIGGER memory_no_content_transition BEFORE UPDATE OF state ON memory WHEN NEW.state NOT IN ('source_blocked','deletion_pending')
BEGIN SELECT RAISE(ABORT,'content capability unavailable'); END;
CREATE TRIGGER deletion_no_completion_insert BEFORE INSERT ON deletion_request WHEN NEW.status='completed'
BEGIN SELECT RAISE(ABORT,'erasure capability unavailable'); END;
CREATE TRIGGER deletion_no_completion_update BEFORE UPDATE ON deletion_request WHEN NEW.status='completed'
BEGIN SELECT RAISE(ABORT,'erasure capability unavailable'); END;
CREATE TRIGGER receipt_no_verification_insert BEFORE INSERT ON erasure_receipt WHEN NEW.status='verified'
BEGIN SELECT RAISE(ABORT,'erasure capability unavailable'); END;
CREATE TRIGGER receipt_no_verification_update BEFORE UPDATE ON erasure_receipt WHEN NEW.status='verified'
BEGIN SELECT RAISE(ABORT,'erasure capability unavailable'); END;
CREATE TRIGGER event_immutable_update BEFORE UPDATE ON event BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER event_immutable_delete BEFORE DELETE ON event BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER evidence_immutable_update BEFORE UPDATE ON evidence BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER evidence_immutable_delete BEFORE DELETE ON evidence BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER revision_immutable_update BEFORE UPDATE ON revision BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER revision_immutable_delete BEFORE DELETE ON revision BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER revision_evidence_immutable_update BEFORE UPDATE ON revision_evidence BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER revision_evidence_immutable_delete BEFORE DELETE ON revision_evidence BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER invalidation_immutable_update BEFORE UPDATE ON revision_invalidation BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER invalidation_immutable_delete BEFORE DELETE ON revision_invalidation BEGIN SELECT RAISE(ABORT,'immutable'); END;
-- Reverse ownership has one canonical source, never a separately maintained guessed refcount.
CREATE VIEW payload_owner AS
 SELECT tenant_id,space_id,payload_id,'event' AS owner_kind,id AS owner_id FROM event WHERE payload_id IS NOT NULL
 UNION ALL SELECT tenant_id,space_id,payload_id,'evidence',id FROM evidence
 UNION ALL SELECT tenant_id,space_id,locator_payload_id,'evidence',id FROM evidence
 UNION ALL SELECT tenant_id,space_id,content_payload_id,'revision',id FROM revision;
CREATE INDEX evidence_by_event ON evidence(tenant_id,space_id,event_id);
CREATE INDEX revision_evidence_reverse ON revision_evidence(tenant_id,space_id,evidence_id);
CREATE INDEX revision_by_memory ON revision(tenant_id,space_id,memory_id,number);
CREATE INDEX outbox_claim ON outbox_job(status,next_run_at_ms);

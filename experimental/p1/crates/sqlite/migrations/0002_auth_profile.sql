-- Auth profile 2, isolated synthetic metadata only. No content or bearer credentials.
CREATE TABLE auth_instance (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), realm_id TEXT NOT NULL UNIQUE CHECK(length(realm_id)=36)
) STRICT;
CREATE TABLE identity_binding (
 subject_id TEXT PRIMARY KEY CHECK(length(subject_id)=36), tenant_id TEXT NOT NULL, principal_id TEXT NOT NULL,
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TABLE auth_space (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 PRIMARY KEY(tenant_id,space_id), FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id)
) STRICT;
CREATE TABLE direct_grant (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, principal_id TEXT NOT NULL,
 capability TEXT NOT NULL CHECK(capability='metadata.append'),
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), expires_at_ms INTEGER NOT NULL CHECK(expires_at_ms>0),
 PRIMARY KEY(tenant_id,space_id,principal_id,capability),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TABLE metadata_marker (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL CHECK(length(id)=36),
 version INTEGER NOT NULL CHECK(version>0), current_event_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,current_event_id) REFERENCES event(tenant_id,space_id,id)
) STRICT;
CREATE TABLE metadata_retry (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, principal_id TEXT NOT NULL, key_id TEXT NOT NULL CHECK(length(key_id)=36),
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 version INTEGER NOT NULL CHECK(version>0), knowledge_seq INTEGER NOT NULL CHECK(knowledge_seq>0), event_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,principal_id,key_id),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq),
 FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id)
) STRICT;

CREATE TABLE auth_audit (
 tenant_id TEXT NOT NULL, epoch INTEGER NOT NULL CHECK(epoch>0),
 action TEXT NOT NULL CHECK(action IN ('tenant.bootstrap','principal.state','tenant.state','space.create','space.state','grant.change','identity.state')),
 recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms>=0),
 PRIMARY KEY(tenant_id,epoch), FOREIGN KEY(tenant_id) REFERENCES tenant(id)
) STRICT;
CREATE TABLE metadata_audit (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, event_id TEXT NOT NULL,
 action TEXT NOT NULL CHECK(action='metadata.append'),
 PRIMARY KEY(tenant_id,event_id), FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id)
) STRICT;
CREATE TABLE metadata_outbox (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, event_id TEXT NOT NULL,
 knowledge_seq INTEGER NOT NULL, status TEXT NOT NULL CHECK(status='pending'),
 PRIMARY KEY(tenant_id,event_id), FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq)
) STRICT;
CREATE TRIGGER auth_audit_immutable_update BEFORE UPDATE ON auth_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER auth_audit_immutable_delete BEFORE DELETE ON auth_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER metadata_audit_immutable_update BEFORE UPDATE ON metadata_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER metadata_audit_immutable_delete BEFORE DELETE ON metadata_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;

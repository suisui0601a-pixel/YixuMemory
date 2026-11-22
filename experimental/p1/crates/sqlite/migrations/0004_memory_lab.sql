-- Fresh, private M1 profile ONLY. Existing v1/v2/v3 files are NOT migrated.
-- These grants cannot authorize available Payloads or active Memories.
CREATE TABLE memory_grant (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, principal_id TEXT NOT NULL,
 capability TEXT NOT NULL CHECK(capability IN ('memory.append','memory.revise')),
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), expires_at_ms INTEGER NOT NULL CHECK(expires_at_ms>0),
 PRIMARY KEY(tenant_id,space_id,principal_id,capability),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TABLE memory_retry (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, principal_id TEXT NOT NULL,
 key_id TEXT NOT NULL CHECK(length(key_id)=36),
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 memory_id TEXT NOT NULL, revision_id TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version>0), number INTEGER NOT NULL CHECK(number>0),
 knowledge_seq INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,principal_id,key_id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,memory_id,revision_id,number)
 REFERENCES revision(tenant_id,space_id,memory_id,id,number),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq)
) STRICT;
CREATE TABLE memory_audit (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, revision_id TEXT NOT NULL,
 principal_id TEXT NOT NULL, action TEXT NOT NULL CHECK(action IN ('memory.append','memory.revise')),
 PRIMARY KEY(tenant_id,revision_id),
 FOREIGN KEY(tenant_id,space_id,revision_id) REFERENCES revision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TRIGGER memory_audit_immutable_update BEFORE UPDATE ON memory_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER memory_audit_immutable_delete BEFORE DELETE ON memory_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;

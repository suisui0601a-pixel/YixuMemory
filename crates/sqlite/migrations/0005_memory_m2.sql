-- Fresh synthetic Profile 5 only. NEVER applied to an existing Profile 1/2/3/4.
DROP TABLE memory_grant;
CREATE TABLE memory_grant (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,principal_id TEXT NOT NULL,
 capability TEXT NOT NULL CHECK(capability IN ('memory.append','memory.revise','memory.read','memory.history','memory.search')),
 enabled INTEGER NOT NULL CHECK(enabled IN(0,1)),expires_at_ms INTEGER NOT NULL CHECK(expires_at_ms>0),
 PRIMARY KEY(tenant_id,space_id,principal_id,capability),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TABLE revision_time (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,revision_id TEXT NOT NULL,
 memory_version_at_revision INTEGER NOT NULL CHECK(memory_version_at_revision>0),
 known INTEGER NOT NULL CHECK(known IN(0,1)),
 from_min INTEGER,from_max INTEGER,from_precision INTEGER,
 to_min INTEGER,to_max INTEGER,to_precision INTEGER,
 PRIMARY KEY(tenant_id,space_id,revision_id),
 FOREIGN KEY(tenant_id,space_id,revision_id) REFERENCES revision(tenant_id,space_id,id),
 CHECK((known=0 AND from_min IS NULL AND from_max IS NULL AND from_precision IS NULL AND to_min IS NULL AND to_max IS NULL AND to_precision IS NULL)
 OR (known=1 AND from_min IS NOT NULL AND from_max IS NOT NULL AND from_min<=from_max AND from_precision IS NOT NULL AND from_precision IN(0,1,2,3,4)
 AND (from_precision<>0 OR from_min=from_max)
 AND ((to_min IS NULL AND to_max IS NULL AND to_precision IS NULL)
 OR (to_min IS NOT NULL AND to_max IS NOT NULL AND to_min<=to_max AND to_precision IS NOT NULL AND to_precision IN(0,1,2,3,4) AND from_max<to_min AND (to_precision<>0 OR to_min=to_max)))))
) STRICT;
CREATE TABLE conflict (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,id TEXT NOT NULL,
 left_revision_id TEXT NOT NULL,right_revision_id TEXT NOT NULL,knowledge_seq INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,space_id,id),UNIQUE(tenant_id,space_id,left_revision_id,right_revision_id),
 CHECK(left_revision_id<right_revision_id),
 FOREIGN KEY(tenant_id,space_id,left_revision_id) REFERENCES revision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,right_revision_id) REFERENCES revision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq)
) STRICT;
CREATE TABLE conflict_decision (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,id TEXT NOT NULL,conflict_id TEXT NOT NULL,
 number INTEGER NOT NULL CHECK(number>0),parent_number INTEGER,
 status TEXT NOT NULL CHECK(status IN('open','resolved')),resolution_revision_id TEXT,basis_revision_id TEXT,
 event_id TEXT NOT NULL,knowledge_seq INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,space_id,id),UNIQUE(tenant_id,space_id,conflict_id,number),
 FOREIGN KEY(tenant_id,space_id,conflict_id) REFERENCES conflict(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,conflict_id,parent_number) REFERENCES conflict_decision(tenant_id,space_id,conflict_id,number),
 FOREIGN KEY(tenant_id,space_id,resolution_revision_id) REFERENCES revision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,basis_revision_id) REFERENCES revision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq),
 CHECK((number=1 AND parent_number IS NULL AND status='open' AND basis_revision_id IS NULL)
 OR (number>1 AND parent_number IS NOT NULL AND parent_number=number-1)),
 CHECK((status='open' AND resolution_revision_id IS NULL) OR(status='resolved' AND resolution_revision_id IS NOT NULL AND basis_revision_id IS NULL)),
 CHECK(number=1 OR status='resolved' OR basis_revision_id IS NOT NULL)
) STRICT;
CREATE TABLE conflict_retry (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,principal_id TEXT NOT NULL,key_id TEXT NOT NULL,
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 conflict_id TEXT NOT NULL,decision_id TEXT NOT NULL,version INTEGER NOT NULL,knowledge_seq INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,principal_id,key_id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,space_id,conflict_id,version) REFERENCES conflict_decision(tenant_id,space_id,conflict_id,number),
 FOREIGN KEY(tenant_id,space_id,decision_id) REFERENCES conflict_decision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq)
) STRICT;
CREATE TABLE conflict_audit (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,decision_id TEXT NOT NULL,principal_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,decision_id),
 FOREIGN KEY(tenant_id,space_id,decision_id) REFERENCES conflict_decision(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TABLE conflict_outbox (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,decision_id TEXT NOT NULL,status TEXT NOT NULL CHECK(status='pending'),
 PRIMARY KEY(tenant_id,decision_id),FOREIGN KEY(tenant_id,space_id,decision_id) REFERENCES conflict_decision(tenant_id,space_id,id)
) STRICT;
CREATE TABLE metadata_read_audit (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,id TEXT NOT NULL,principal_id TEXT NOT NULL,
 snapshot_seq INTEGER NOT NULL CHECK(snapshot_seq>=0),query_digest BLOB NOT NULL CHECK(length(query_digest)=32),recorded_at_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TRIGGER revision_time_no_update BEFORE UPDATE ON revision_time BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER revision_time_no_delete BEFORE DELETE ON revision_time BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER conflict_no_update BEFORE UPDATE ON conflict BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER conflict_no_delete BEFORE DELETE ON conflict BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER decision_no_update BEFORE UPDATE ON conflict_decision BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER decision_no_delete BEFORE DELETE ON conflict_decision BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER conflict_audit_no_update BEFORE UPDATE ON conflict_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER conflict_audit_no_delete BEFORE DELETE ON conflict_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER read_audit_no_update BEFORE UPDATE ON metadata_read_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER read_audit_no_delete BEFORE DELETE ON metadata_read_audit BEGIN SELECT RAISE(ABORT,'immutable'); END;

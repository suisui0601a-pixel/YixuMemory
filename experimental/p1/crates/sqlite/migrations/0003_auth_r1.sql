-- Auth R1 profile 3, FRESH synthetic DB only, composed after unchanged profile 2.
-- Receipt must retain server-created marker as well as event for response-loss replay.
-- No online upgrade/import from existing v1/v2 databases is provided.
CREATE UNIQUE INDEX metadata_marker_scoped_identity ON metadata_marker(tenant_id,space_id,id);
DROP TABLE metadata_retry;
CREATE TABLE metadata_retry (
 tenant_id TEXT NOT NULL, space_id TEXT NOT NULL, principal_id TEXT NOT NULL, key_id TEXT NOT NULL CHECK(length(key_id)=36),
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 version INTEGER NOT NULL CHECK(version>0), knowledge_seq INTEGER NOT NULL CHECK(knowledge_seq>0),
 event_id TEXT NOT NULL, marker_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,principal_id,key_id),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id),
 FOREIGN KEY(tenant_id,knowledge_seq) REFERENCES commit_record(tenant_id,knowledge_seq),
 FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id),
 FOREIGN KEY(tenant_id,space_id,marker_id) REFERENCES metadata_marker(tenant_id,space_id,id)
) STRICT;

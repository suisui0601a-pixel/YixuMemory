-- Fresh isolated Profile7 only; never upgrades existing files.
CREATE TABLE local_key_registry (
 tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,payload_id TEXT NOT NULL,key_id TEXT NOT NULL,
 binding BLOB NOT NULL CHECK(length(binding)=163),wrapped_hash BLOB NOT NULL CHECK(length(wrapped_hash)=32),
 revoked INTEGER NOT NULL CHECK(revoked IN(0,1)),
 PRIMARY KEY(tenant_id,key_id),UNIQUE(tenant_id,payload_id),
 FOREIGN KEY(tenant_id,space_id,payload_id) REFERENCES sealed_copy(tenant_id,space_id,payload_id)
) STRICT;
CREATE TRIGGER local_key_monotonic BEFORE UPDATE ON local_key_registry
WHEN NEW.tenant_id<>OLD.tenant_id OR NEW.space_id<>OLD.space_id OR NEW.payload_id<>OLD.payload_id
 OR NEW.key_id<>OLD.key_id OR NEW.binding<>OLD.binding OR NEW.wrapped_hash<>OLD.wrapped_hash OR NEW.revoked<OLD.revoked
BEGIN SELECT RAISE(ABORT,'immutable key binding'); END;
CREATE TRIGGER local_key_no_delete BEFORE DELETE ON local_key_registry BEGIN SELECT RAISE(ABORT,'retain tombstone'); END;
CREATE TABLE local_key_gate(singleton INTEGER PRIMARY KEY CHECK(singleton=1),state TEXT NOT NULL CHECK(state IN('ready','unknown')) ) STRICT;
INSERT INTO local_key_gate VALUES(1,'ready');

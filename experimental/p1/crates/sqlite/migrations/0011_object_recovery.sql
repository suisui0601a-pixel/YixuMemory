-- Explicit fresh Profile 11 only. The base Profile 10 schema is installed first.
PRAGMA user_version=11;
CREATE TABLE object_quarantine(
 operation BLOB PRIMARY KEY REFERENCES write_intent(operation),
 reason TEXT NOT NULL CHECK(reason IN ('Integrity','PayloadUnavailable','KeyUnavailable','Dependency')),
 incarnation BLOB NOT NULL CHECK(length(incarnation)=16)
) STRICT;
CREATE TABLE deletion_intent(
 event INTEGER PRIMARY KEY REFERENCES security_event(id),
 scope BLOB NOT NULL CHECK(length(scope)=48),
 principal BLOB NOT NULL CHECK(length(principal)=16),
 request BLOB NOT NULL CHECK(length(request)=16 AND request<>zeroblob(16)),
 kind TEXT NOT NULL CHECK(kind IN ('Memory','Source')),
 target BLOB NOT NULL CHECK(length(target)=16),
 UNIQUE(scope,principal,request)
) STRICT;
CREATE TABLE deletion_member(
 event INTEGER NOT NULL REFERENCES deletion_intent(event),
 operation BLOB NOT NULL REFERENCES write_intent(operation),
 PRIMARY KEY(event,operation)
) STRICT;
CREATE TABLE copy_manifest_b3(
 operation BLOB PRIMARY KEY REFERENCES publication(operation),
 generation INTEGER NOT NULL CHECK(generation=1)
) STRICT;
CREATE TABLE backup_registration(
 operation BLOB NOT NULL REFERENCES write_intent(operation),
 backup BLOB NOT NULL CHECK(length(backup)=16),
 generation INTEGER NOT NULL CHECK(generation>0),
 artifact_hash BLOB NOT NULL CHECK(length(artifact_hash)=32),
 artifact_identity BLOB NOT NULL CHECK(length(artifact_identity)=12),
 PRIMARY KEY(operation,backup,generation)
) STRICT;
CREATE TABLE backup_receipt(
 operation BLOB NOT NULL,
 backup BLOB NOT NULL,
 generation INTEGER NOT NULL,
 PRIMARY KEY(operation,backup,generation),
 FOREIGN KEY(operation,backup,generation) REFERENCES backup_registration(operation,backup,generation)
) STRICT;
CREATE TABLE deletion_work(
 operation BLOB PRIMARY KEY REFERENCES write_intent(operation),
 event INTEGER NOT NULL REFERENCES deletion_intent(event),
 generation INTEGER NOT NULL CHECK(generation=1),
 marker BLOB NOT NULL,
 marker_identity BLOB CHECK(marker_identity IS NULL OR length(marker_identity)=12)
) STRICT;
CREATE TABLE deletion_observation(
 operation BLOB NOT NULL REFERENCES deletion_work(operation),
 action TEXT NOT NULL CHECK(action IN ('KeyUnavailable','CiphertextRemoved','WrapperRemoved')),
 generation INTEGER NOT NULL CHECK(generation=1),
 PRIMARY KEY(operation,action),
 UNIQUE(operation,action,generation)
) STRICT;
CREATE TABLE deletion_receipt(
 operation BLOB NOT NULL,
 action TEXT NOT NULL,
 generation INTEGER NOT NULL,
 PRIMARY KEY(operation,action),
 FOREIGN KEY(operation,action,generation) REFERENCES deletion_observation(operation,action,generation)
) STRICT;
CREATE VIEW content_outbox_v2 AS
 SELECT o.operation,o.knowledge_seq,2 AS version,
 CASE WHEN v.number=1 THEN 'MemoryCreated' ELSE 'RevisionCreated' END AS action,
 v.memory,v.id AS revision,v.number
 FROM outbox o JOIN receipt r USING(operation) JOIN revision v
 ON v.scope=r.scope AND v.memory=r.memory AND v.id=r.revision;
CREATE TRIGGER b3_copy_registered AFTER INSERT ON payload
BEGIN INSERT INTO copy_manifest_b3 VALUES(NEW.operation,NEW.generation); END;
CREATE TRIGGER b3_backup_fenced BEFORE INSERT ON backup_registration
WHEN EXISTS(SELECT 1 FROM deletion_member WHERE operation=NEW.operation)
 OR EXISTS(SELECT 1 FROM security_event WHERE state<>'Applied')
BEGIN SELECT RAISE(ABORT,'backup registration blocked by deletion'); END;
CREATE TRIGGER b3_backup_after_key BEFORE INSERT ON backup_receipt
WHEN NOT EXISTS(SELECT 1 FROM deletion_receipt WHERE operation=NEW.operation AND action='KeyUnavailable')
BEGIN SELECT RAISE(ABORT,'backup expiry requires key revocation receipt'); END;
CREATE TRIGGER b3_quarantine_committed BEFORE INSERT ON object_quarantine
WHEN NOT EXISTS(SELECT 1 FROM write_intent WHERE operation=NEW.operation AND state='Committed')
BEGIN SELECT RAISE(ABORT,'unknown write cannot be downgraded to object isolation'); END;
CREATE TRIGGER b3_member_fenced BEFORE INSERT ON deletion_member
WHEN NOT EXISTS(SELECT 1 FROM deletion_intent d JOIN security_event e ON e.id=d.event
 JOIN write_intent i ON i.operation=NEW.operation
 WHERE d.event=NEW.event AND e.state='EffectiveFence' AND d.scope=i.scope)
BEGIN SELECT RAISE(ABORT,'deletion closure requires matching fence'); END;
CREATE TRIGGER b3_work_fenced BEFORE INSERT ON deletion_work
WHEN NOT EXISTS(SELECT 1 FROM deletion_member m JOIN security_event e ON e.id=m.event
 WHERE m.event=NEW.event AND m.operation=NEW.operation AND e.state='EffectiveFence')
BEGIN SELECT RAISE(ABORT,'work requires frozen deletion member'); END;
CREATE TRIGGER b3_work_identity BEFORE UPDATE ON deletion_work
WHEN NEW.operation<>OLD.operation OR NEW.event<>OLD.event OR NEW.generation<>OLD.generation
 OR NEW.marker<>OLD.marker OR OLD.marker_identity IS NOT NULL OR NEW.marker_identity IS NULL
BEGIN SELECT RAISE(ABORT,'immutable work identity'); END;
CREATE TRIGGER b3_observation_order BEFORE INSERT ON deletion_observation
WHEN NOT EXISTS(SELECT 1 FROM deletion_work w JOIN security_event e ON e.id=w.event
 WHERE w.operation=NEW.operation AND w.generation=NEW.generation AND e.state='Applied')
 OR (NEW.action<>'KeyUnavailable' AND NOT EXISTS(
 SELECT 1 FROM deletion_receipt WHERE operation=NEW.operation AND action='KeyUnavailable'))
 OR (NEW.action='KeyUnavailable' AND NOT EXISTS(
 SELECT 1 FROM deletion_work WHERE operation=NEW.operation AND marker_identity IS NOT NULL))
BEGIN SELECT RAISE(ABORT,'cleanup observation has no applied work or key receipt'); END;

CREATE TABLE deletion_closure(
 event INTEGER NOT NULL REFERENCES deletion_intent(event),
 operation BLOB NOT NULL REFERENCES write_intent(operation),
 PRIMARY KEY(event,operation)
) STRICT;
CREATE TRIGGER b3_closure_initial BEFORE INSERT ON deletion_closure
WHEN NOT EXISTS(SELECT 1 FROM deletion_intent d JOIN security_event e ON e.id=d.event JOIN write_intent i ON i.operation=NEW.operation WHERE d.event=NEW.event AND e.state='Closing' AND d.scope=i.scope AND i.state='Committed')
BEGIN SELECT RAISE(ABORT,'closure requires matching Closing'); END;

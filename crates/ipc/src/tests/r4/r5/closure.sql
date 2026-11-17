PRAGMA application_id = 1498952245;

CREATE TABLE r5_object (
    id BLOB PRIMARY KEY CHECK(length(id)=16 AND id<>zeroblob(16)),
    kind TEXT NOT NULL CHECK(kind IN ('Memory','Revision','Source','Evidence','Payload','Key','Copy','Backup')),
    generation INTEGER NOT NULL CHECK(generation>0),
    parent BLOB REFERENCES r5_object(id),
    UNIQUE(id,generation)
) STRICT;
CREATE TABLE r5_evidence (
    revision BLOB NOT NULL REFERENCES r5_object(id),
    evidence BLOB NOT NULL REFERENCES r5_object(id),
    PRIMARY KEY(revision,evidence)
) STRICT;
CREATE TABLE r5_current (
    memory BLOB PRIMARY KEY REFERENCES r5_object(id),
    revision BLOB NOT NULL REFERENCES r5_object(id)
) STRICT;

CREATE TABLE r5_command (
    event INTEGER PRIMARY KEY REFERENCES security_event(id),
    request BLOB NOT NULL UNIQUE CHECK(length(request)=16 AND request<>zeroblob(16)),
    body BLOB NOT NULL CHECK(length(body)=57)
) STRICT;
CREATE UNIQUE INDEX r5_state_ref ON security_event(id,state);
CREATE TABLE r5_applied (
    event INTEGER PRIMARY KEY REFERENCES r5_apply_context(event),
    epoch INTEGER NOT NULL UNIQUE CHECK(epoch>7),
    phase TEXT NOT NULL DEFAULT 'Applied' CHECK(phase='Applied'),
    FOREIGN KEY(event,phase) REFERENCES security_event(id,state) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r5_apply_context (
    event INTEGER PRIMARY KEY REFERENCES r5_command(event),
    FOREIGN KEY(event) REFERENCES r5_applied(event) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r5_tombstone (
    object BLOB NOT NULL,
    generation INTEGER NOT NULL,
    event INTEGER NOT NULL REFERENCES r5_applied(event) DEFERRABLE INITIALLY DEFERRED,
    PRIMARY KEY(object,generation),
    FOREIGN KEY(object,generation) REFERENCES r5_object(id,generation)
) STRICT;
CREATE TABLE r5_outbox (
    object BLOB NOT NULL,
    generation INTEGER NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('KeyUnavailable','CiphertextRemoved','BackupExpired')),
    PRIMARY KEY(object,generation),
    UNIQUE(object,generation,action),
    FOREIGN KEY(object,generation) REFERENCES r5_tombstone(object,generation)
) STRICT;
CREATE TABLE r5_plan (
    event INTEGER NOT NULL REFERENCES r5_applied(event) DEFERRABLE INITIALLY DEFERRED,
    object BLOB NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY(event,object,generation),
    FOREIGN KEY(object,generation) REFERENCES r5_outbox(object,generation)
) STRICT;
CREATE TABLE r5_observation (
    object BLOB NOT NULL,
    generation INTEGER NOT NULL,
    action TEXT NOT NULL,
    PRIMARY KEY(object,generation),
    UNIQUE(object,generation,action),
    FOREIGN KEY(object,generation,action) REFERENCES r5_outbox(object,generation,action)
) STRICT;
CREATE TABLE r5_receipt (
    object BLOB NOT NULL,
    generation INTEGER NOT NULL,
    action TEXT NOT NULL,
    PRIMARY KEY(object,generation),
    FOREIGN KEY(object,generation,action) REFERENCES r5_observation(object,generation,action)
) STRICT;

CREATE TRIGGER r5_require_command BEFORE UPDATE OF state ON security_event
WHEN NOT EXISTS(SELECT 1 FROM r5_command WHERE event=NEW.id)
  OR (NEW.state='Applied' AND NOT EXISTS(SELECT 1 FROM r5_applied WHERE event=NEW.id))
BEGIN SELECT RAISE(ABORT,'security change lacks its transaction record'); END;

CREATE TRIGGER r5_apply_fenced BEFORE INSERT ON r5_applied
WHEN (SELECT state FROM security_event WHERE id=NEW.event) IS NOT 'EffectiveFence'
  OR (SELECT epoch FROM authority) IS NOT NEW.epoch
BEGIN SELECT RAISE(ABORT,'security change lacks effective fence or epoch'); END;

CREATE TRIGGER r5_context_fenced BEFORE INSERT ON r5_apply_context
WHEN (SELECT state FROM security_event WHERE id=NEW.event) IS NOT 'EffectiveFence'
BEGIN SELECT RAISE(ABORT,'security write requires effective fence'); END;

CREATE VIEW r5_active_change AS
SELECT c.event FROM r5_apply_context c JOIN security_event e ON e.id=c.event
WHERE e.state='EffectiveFence' AND NOT EXISTS(SELECT 1 FROM r5_applied a WHERE a.event=c.event);

CREATE TRIGGER r5_current_update BEFORE UPDATE ON r5_current
WHEN NOT EXISTS(SELECT 1 FROM r5_active_change) OR NEW.memory IS NOT OLD.memory
 OR NOT EXISTS(
    SELECT 1 FROM r5_object newer JOIN r5_object older
    WHERE newer.id=NEW.revision AND older.id=OLD.revision AND newer.kind='Revision'
      AND newer.parent=OLD.memory AND newer.generation>older.generation
 )
BEGIN SELECT RAISE(ABORT,'current revision requires ordered forward transition'); END;

CREATE TRIGGER r5_tombstone_fenced BEFORE INSERT ON r5_tombstone
WHEN NOT EXISTS(SELECT 1 FROM r5_active_change WHERE event=NEW.event)
BEGIN SELECT RAISE(ABORT,'tombstone requires ordered apply'); END;

CREATE TRIGGER r5_plan_fenced BEFORE INSERT ON r5_plan
WHEN NOT EXISTS(SELECT 1 FROM r5_active_change WHERE event=NEW.event)
BEGIN SELECT RAISE(ABORT,'cleanup plan requires ordered apply'); END;

CREATE TRIGGER r5_action_typed BEFORE INSERT ON r5_outbox
WHEN NOT EXISTS(SELECT 1 FROM r5_active_change)
 OR (SELECT kind FROM r5_object WHERE id=NEW.object) IS NOT
    CASE NEW.action WHEN 'KeyUnavailable' THEN 'Key' WHEN 'CiphertextRemoved' THEN 'Copy' ELSE 'Backup' END
BEGIN SELECT RAISE(ABORT,'cleanup action does not match object type'); END;

CREATE TRIGGER r5_ciphertext_after_key BEFORE INSERT ON r5_observation
WHEN NEW.action='CiphertextRemoved' AND NOT EXISTS (
    SELECT 1 FROM r5_object copy JOIN r5_object key ON key.parent=copy.parent AND key.kind='Key'
    JOIN r5_receipt receipt ON receipt.object=key.id AND receipt.generation=key.generation
    WHERE copy.id=NEW.object AND receipt.action='KeyUnavailable'
)
BEGIN SELECT RAISE(ABORT,'ciphertext cleanup requires key unavailability receipt'); END;

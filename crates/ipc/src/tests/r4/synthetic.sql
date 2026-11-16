CREATE TABLE runtime(
    id INTEGER PRIMARY KEY CHECK(id=1),
    profile TEXT NOT NULL CHECK(profile='R4-SYNTHETIC-1'),
    incarnation BLOB NOT NULL CHECK(length(incarnation)=16),
    blocked INTEGER NOT NULL DEFAULT 0 CHECK(blocked IN (0,1))
) STRICT;
CREATE TRIGGER runtime_monotone BEFORE UPDATE ON runtime
WHEN NEW.id<>OLD.id OR NEW.profile<>OLD.profile OR NEW.incarnation<>OLD.incarnation
     OR NEW.blocked<OLD.blocked
BEGIN SELECT RAISE(ABORT,'immutable runtime identity or block'); END;

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

CREATE TABLE release_intent(
    operation INTEGER PRIMARY KEY,
    attempt INTEGER NOT NULL DEFAULT 1 CHECK(attempt=1),
    scope BLOB NOT NULL,
    principal BLOB NOT NULL,
    target BLOB NOT NULL,
    request BLOB NOT NULL CHECK(length(request)=16),
    binding BLOB NOT NULL CHECK(length(binding)=32),
    state TEXT NOT NULL DEFAULT 'Armed'
        CHECK(state IN ('Armed','CancelRequested','Unknown','Settled')),
    UNIQUE(scope,principal,request),
    UNIQUE(operation,attempt,binding),
    FOREIGN KEY(scope,principal,target) REFERENCES authority(scope,principal,target)
) STRICT;
CREATE UNIQUE INDEX one_unsettled ON release_intent((1)) WHERE state<>'Settled';

CREATE TABLE observation(
    operation INTEGER PRIMARY KEY,
    attempt INTEGER NOT NULL,
    binding BLOB NOT NULL,
    outcome TEXT NOT NULL CHECK(outcome IN ('SyntheticAccepted','SyntheticCancelled')),
    UNIQUE(operation,attempt,binding,outcome),
    FOREIGN KEY(operation,attempt,binding) REFERENCES release_intent(operation,attempt,binding)
) STRICT;
CREATE TABLE receipt(
    operation INTEGER PRIMARY KEY,
    attempt INTEGER NOT NULL,
    binding BLOB NOT NULL,
    outcome TEXT NOT NULL,
    FOREIGN KEY(operation,attempt,binding,outcome)
        REFERENCES observation(operation,attempt,binding,outcome)
) STRICT;
CREATE TABLE audit(
    sequence INTEGER PRIMARY KEY,
    operation INTEGER NOT NULL REFERENCES release_intent(operation),
    event TEXT NOT NULL CHECK(event IN ('Armed','Observed','Settled')),
    UNIQUE(operation,event)
) STRICT;

CREATE TABLE security_event(
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK(kind IN
        ('Grant','Revocation','Delete','Revision','Source','Key','Copy','Principal','Space')),
    scope BLOB NOT NULL CHECK(length(scope)=48),
    state TEXT NOT NULL DEFAULT 'Closing' CHECK(state IN ('Closing','EffectiveFence','Applied'))
) STRICT;
CREATE TRIGGER security_initial BEFORE INSERT ON security_event
WHEN NEW.state<>'Closing' OR (SELECT blocked FROM runtime WHERE id=1) IS NOT 0
BEGIN SELECT RAISE(ABORT,'security event requires live owner'); END;
CREATE TRIGGER security_progress BEFORE UPDATE ON security_event
WHEN NEW.id<>OLD.id OR NEW.kind<>OLD.kind OR NEW.scope<>OLD.scope
 OR NOT ((OLD.state='Closing' AND NEW.state='EffectiveFence')
      OR (OLD.state='EffectiveFence' AND NEW.state='Applied'))
 OR EXISTS(SELECT 1 FROM release_intent WHERE state<>'Settled')
 OR EXISTS(SELECT 1 FROM security_event WHERE id<OLD.id AND state<>'Applied')
 OR (SELECT blocked FROM runtime WHERE id=1) IS NOT 0
BEGIN SELECT RAISE(ABORT,'security event must drain in order'); END;
CREATE TRIGGER keep_security_event BEFORE DELETE ON security_event
BEGIN SELECT RAISE(ABORT,'immutable security history'); END;

CREATE TRIGGER dispatch_guard BEFORE INSERT ON release_intent
WHEN NEW.state<>'Armed' OR (SELECT blocked FROM runtime WHERE id=1) IS NOT 0
 OR EXISTS(SELECT 1 FROM security_event WHERE state<>'Applied')
BEGIN SELECT RAISE(ABORT,'dispatch blocked'); END;
CREATE TRIGGER dispatch_audit AFTER INSERT ON release_intent
BEGIN INSERT INTO audit(operation,event) VALUES(NEW.operation,'Armed'); END;
CREATE TRIGGER release_progress BEFORE UPDATE ON release_intent
WHEN NEW.operation<>OLD.operation OR NEW.attempt<>OLD.attempt OR NEW.scope<>OLD.scope
 OR NEW.principal<>OLD.principal OR NEW.target<>OLD.target OR NEW.request<>OLD.request
 OR NEW.binding<>OLD.binding OR NOT (
      (OLD.state='Armed' AND NEW.state='CancelRequested')
      OR (OLD.state IN ('Armed','CancelRequested') AND NEW.state='Unknown')
      OR (OLD.state IN ('Armed','CancelRequested','Unknown') AND NEW.state='Settled'
          AND EXISTS(SELECT 1 FROM receipt WHERE operation=OLD.operation))
 )
BEGIN SELECT RAISE(ABORT,'invalid intent transition'); END;
CREATE TRIGGER keep_release BEFORE DELETE ON release_intent
BEGIN SELECT RAISE(ABORT,'immutable intent history'); END;

CREATE TRIGGER observation_guard BEFORE INSERT ON observation
WHEN (SELECT blocked FROM runtime WHERE id=1) IS NOT 0
 OR NOT EXISTS(SELECT 1 FROM release_intent WHERE operation=NEW.operation AND state<>'Settled')
BEGIN SELECT RAISE(ABORT,'unobserved operation'); END;
CREATE TRIGGER observation_audit AFTER INSERT ON observation
BEGIN INSERT INTO audit(operation,event) VALUES(NEW.operation,'Observed'); END;
CREATE TRIGGER observation_no_update BEFORE UPDATE ON observation
BEGIN SELECT RAISE(ABORT,'immutable observation'); END;
CREATE TRIGGER observation_no_delete BEFORE DELETE ON observation
BEGIN SELECT RAISE(ABORT,'immutable observation'); END;

CREATE TRIGGER receipt_guard BEFORE INSERT ON receipt
WHEN (SELECT blocked FROM runtime WHERE id=1) IS NOT 0
 OR NOT EXISTS(SELECT 1 FROM release_intent WHERE operation=NEW.operation AND state<>'Settled')
BEGIN SELECT RAISE(ABORT,'receipt requires unsettled operation'); END;
CREATE TRIGGER settle AFTER INSERT ON receipt BEGIN
    UPDATE release_intent SET state='Settled' WHERE operation=NEW.operation;
    INSERT INTO audit(operation,event) VALUES(NEW.operation,'Settled');
END;
CREATE TRIGGER receipt_no_update BEFORE UPDATE ON receipt
BEGIN SELECT RAISE(ABORT,'immutable receipt'); END;
CREATE TRIGGER receipt_no_delete BEFORE DELETE ON receipt
BEGIN SELECT RAISE(ABORT,'immutable receipt'); END;
CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit
BEGIN SELECT RAISE(ABORT,'immutable audit'); END;
CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit
BEGIN SELECT RAISE(ABORT,'immutable audit'); END;

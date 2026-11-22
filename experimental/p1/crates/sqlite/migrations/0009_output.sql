-- Fresh synthetic Profile9. No Profile8 upgrade or production release capability.
CREATE TABLE output_runtime(singleton INTEGER PRIMARY KEY CHECK(singleton=1),incarnation TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('RecoveryBlocked','Ready','Stuck')) ) STRICT;
INSERT INTO output_runtime VALUES(1,'unstarted','RecoveryBlocked');
CREATE UNIQUE INDEX release_output_identity ON release_intent(tenant_id,space_id,principal_id,id,incarnation);
CREATE TABLE connection_binding(
 id TEXT PRIMARY KEY,tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,principal_id TEXT NOT NULL,
 incarnation TEXT NOT NULL,destination TEXT NOT NULL,closed INTEGER NOT NULL CHECK(closed IN(0,1)),
 UNIQUE(tenant_id,space_id,principal_id,id,incarnation),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE UNIQUE INDEX connection_one_open ON connection_binding((1)) WHERE closed=0;
CREATE TABLE output_operation(
 id TEXT PRIMARY KEY,tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,principal_id TEXT NOT NULL,
 release_id TEXT NOT NULL,connection_id TEXT NOT NULL,incarnation TEXT NOT NULL,
 attempt INTEGER NOT NULL CHECK(attempt=1),body_length INTEGER NOT NULL CHECK(body_length BETWEEN 1 AND 4096),
 metadata_length INTEGER NOT NULL CHECK(metadata_length BETWEEN 0 AND 512),
 phase TEXT NOT NULL CHECK(phase IN('Prepared','Submitted','CancelRequested','Stuck','Terminal')),
 UNIQUE(release_id),UNIQUE(connection_id),
 FOREIGN KEY(tenant_id,space_id,principal_id,release_id,incarnation) REFERENCES release_intent(tenant_id,space_id,principal_id,id,incarnation),
 FOREIGN KEY(tenant_id,space_id,principal_id,connection_id,incarnation) REFERENCES connection_binding(tenant_id,space_id,principal_id,id,incarnation)
) STRICT;
CREATE UNIQUE INDEX output_one_active ON output_operation((1)) WHERE phase IN('Submitted','CancelRequested','Stuck');
CREATE TRIGGER output_initial BEFORE INSERT ON output_operation WHEN NEW.phase<>'Prepared'
BEGIN SELECT RAISE(ABORT,'must start prepared'); END;
CREATE TRIGGER connection_initial BEFORE INSERT ON connection_binding WHEN NEW.closed<>0
BEGIN SELECT RAISE(ABORT,'must start open'); END;
CREATE TABLE terminal_evidence(
 operation_id TEXT PRIMARY KEY,kind TEXT NOT NULL CHECK(kind IN('Normal','Cancelled','Error')),
 count_kind TEXT NOT NULL CHECK(count_kind IN('Exact','Range')),accepted_min INTEGER NOT NULL CHECK(accepted_min>=0),
 accepted_max INTEGER NOT NULL CHECK(accepted_max>=accepted_min),native_status INTEGER NOT NULL,
 pending TEXT NOT NULL CHECK(pending='NoPending'),
 FOREIGN KEY(operation_id) REFERENCES output_operation(id),
 CHECK(count_kind<>'Exact' OR accepted_min=accepted_max)
) STRICT;
CREATE TRIGGER evidence_valid BEFORE INSERT ON terminal_evidence
WHEN NOT EXISTS(SELECT 1 FROM output_operation o WHERE o.id=NEW.operation_id
 AND o.phase IN('Submitted','CancelRequested','Stuck') AND NEW.accepted_max<=o.body_length)
BEGIN SELECT RAISE(ABORT,'unmatched terminal'); END;
CREATE TRIGGER output_transition BEFORE UPDATE ON output_operation
WHEN NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.space_id<>OLD.space_id OR NEW.principal_id<>OLD.principal_id
 OR NEW.release_id<>OLD.release_id OR NEW.connection_id<>OLD.connection_id OR NEW.incarnation<>OLD.incarnation
 OR NEW.attempt<>OLD.attempt OR NEW.body_length<>OLD.body_length OR NEW.metadata_length<>OLD.metadata_length
 OR NOT ((OLD.phase='Prepared' AND NEW.phase='Submitted')
 OR (OLD.phase='Submitted' AND NEW.phase IN('CancelRequested','Stuck','Terminal'))
 OR (OLD.phase='CancelRequested' AND NEW.phase IN('Stuck','Terminal'))
 OR (OLD.phase='Stuck' AND NEW.phase='Terminal'))
 OR (NEW.phase='Terminal' AND NOT EXISTS(SELECT 1 FROM terminal_evidence WHERE operation_id=OLD.id))
 OR (NEW.phase='Submitted' AND EXISTS(SELECT 1 FROM connection_binding WHERE id=OLD.connection_id AND closed=1))
 OR (NEW.phase='Submitted' AND EXISTS(SELECT 1 FROM security_mutation WHERE state<>'Applied'))
 OR (NEW.phase='Submitted' AND NOT EXISTS(SELECT 1 FROM output_runtime WHERE singleton=1 AND state='Ready' AND incarnation=OLD.incarnation))
BEGIN SELECT RAISE(ABORT,'invalid output transition'); END;
CREATE TRIGGER output_retain BEFORE DELETE ON output_operation BEGIN SELECT RAISE(ABORT,'retain output'); END;
CREATE TRIGGER evidence_immutable BEFORE UPDATE ON terminal_evidence BEGIN SELECT RAISE(ABORT,'immutable terminal'); END;
CREATE TRIGGER evidence_retain BEFORE DELETE ON terminal_evidence BEGIN SELECT RAISE(ABORT,'retain terminal'); END;
CREATE TRIGGER connection_immutable BEFORE UPDATE ON connection_binding
WHEN NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.space_id<>OLD.space_id OR NEW.principal_id<>OLD.principal_id
 OR NEW.incarnation<>OLD.incarnation OR NEW.destination<>OLD.destination OR NEW.closed<=OLD.closed
 OR EXISTS(SELECT 1 FROM output_operation WHERE connection_id=OLD.id AND phase IN('Submitted','CancelRequested','Stuck'))
BEGIN SELECT RAISE(ABORT,'immutable connection'); END;
CREATE TRIGGER connection_retain BEFORE DELETE ON connection_binding BEGIN SELECT RAISE(ABORT,'retain connection'); END;
CREATE TABLE security_mutation(id TEXT PRIMARY KEY,kind TEXT NOT NULL CHECK(kind IN('Grant','Principal','Space','Revision','Source','Key','Deletion','Copy')),
 state TEXT NOT NULL CHECK(state IN('Closing','EffectiveFence','Applied')),incarnation TEXT NOT NULL) STRICT;
CREATE TRIGGER mutation_initial BEFORE INSERT ON security_mutation WHEN NEW.state<>'Closing'
BEGIN SELECT RAISE(ABORT,'must start closing'); END;
CREATE TRIGGER mutation_transition BEFORE UPDATE ON security_mutation
WHEN NEW.id<>OLD.id OR NEW.kind<>OLD.kind OR NEW.incarnation<>OLD.incarnation
 OR NOT ((OLD.state='Closing' AND NEW.state='EffectiveFence') OR (OLD.state='EffectiveFence' AND NEW.state='Applied'))
 OR EXISTS(SELECT 1 FROM output_operation WHERE phase IN('Submitted','CancelRequested','Stuck'))
 OR NOT EXISTS(SELECT 1 FROM output_runtime WHERE singleton=1 AND state='Ready' AND incarnation=OLD.incarnation)
BEGIN SELECT RAISE(ABORT,'active output blocks security change'); END;
CREATE TRIGGER mutation_retain BEFORE DELETE ON security_mutation BEGIN SELECT RAISE(ABORT,'retain mutation'); END;
-- Witness intentionally has no usable anchor, endpoint, issuer or remote implementation.

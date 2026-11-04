-- Fresh, synthetic-only Profile8. No implicit upgrade or production issuer.
CREATE TABLE broker_runtime(singleton INTEGER PRIMARY KEY CHECK(singleton=1),incarnation TEXT NOT NULL,blocked INTEGER NOT NULL CHECK(blocked IN(0,1))) STRICT;
INSERT INTO broker_runtime VALUES(1,'unstarted',1);
CREATE TABLE release_intent(
 id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,principal_id TEXT NOT NULL,
 request_id TEXT NOT NULL,digest BLOB NOT NULL CHECK(length(digest)=32),content_id TEXT NOT NULL,
 binding BLOB NOT NULL CHECK(length(binding)=163),tenant_epoch INTEGER NOT NULL,space_epoch INTEGER NOT NULL,
 purpose TEXT NOT NULL CHECK(purpose='synthetic.read'),destination TEXT NOT NULL,
 expires INTEGER NOT NULL,incarnation TEXT NOT NULL,attempt INTEGER NOT NULL CHECK(attempt IN(0,1)),
 state TEXT NOT NULL CHECK(state IN('Pending','Armed','Accepted','Failed','Partial','Unknown','Denied','Expired','Quarantined')),
 accepted_bytes INTEGER NOT NULL CHECK(accepted_bytes BETWEEN 0 AND 4096),
 UNIQUE(tenant_id,space_id,principal_id,request_id),
 FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id),
 FOREIGN KEY(tenant_id,principal_id) REFERENCES principal(tenant_id,id)
) STRICT;
CREATE TRIGGER release_transition BEFORE UPDATE ON release_intent
WHEN NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.space_id<>OLD.space_id OR NEW.principal_id<>OLD.principal_id
 OR NEW.request_id<>OLD.request_id OR NEW.digest<>OLD.digest OR NEW.content_id<>OLD.content_id OR NEW.binding<>OLD.binding
 OR NEW.tenant_epoch<>OLD.tenant_epoch OR NEW.space_epoch<>OLD.space_epoch OR NEW.purpose<>OLD.purpose
 OR NEW.destination<>OLD.destination OR NEW.expires<>OLD.expires OR NEW.incarnation<>OLD.incarnation
 OR NOT ((OLD.state='Pending' AND NEW.state IN('Armed','Denied','Expired','Quarantined') AND NEW.attempt=CASE WHEN NEW.state='Armed' THEN 1 ELSE 0 END AND NEW.accepted_bytes=0)
 OR (OLD.state='Armed' AND NEW.state IN('Accepted','Failed','Partial','Unknown') AND NEW.attempt=1 AND (NEW.state<>'Failed' OR NEW.accepted_bytes=0)))
BEGIN SELECT RAISE(ABORT,'invalid release transition'); END;
CREATE TRIGGER release_retain BEFORE DELETE ON release_intent BEGIN SELECT RAISE(ABORT,'retain release result'); END;
CREATE TABLE release_event(seq INTEGER PRIMARY KEY,intent_id TEXT NOT NULL,event TEXT NOT NULL,FOREIGN KEY(intent_id) REFERENCES release_intent(id)) STRICT;
CREATE TABLE release_fence(id TEXT PRIMARY KEY,tenant_id TEXT NOT NULL,space_id TEXT NOT NULL,FOREIGN KEY(tenant_id,space_id) REFERENCES space(tenant_id,id)) STRICT;
CREATE TRIGGER release_fence_retain BEFORE DELETE ON release_fence BEGIN SELECT RAISE(ABORT,'retain fence'); END;

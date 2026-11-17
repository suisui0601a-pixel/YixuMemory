//! R5 security ordering for a fresh, metadata-only synthetic realm.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const APPLICATION_ID: i64 = 1_498_952_245;
const SCHEMA: &str = include_str!("r5/closure.sql");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Memory,
    Revision,
    Source,
    Evidence,
    Payload,
    Key,
    Copy,
    Backup,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Memory => "Memory",
            Self::Revision => "Revision",
            Self::Source => "Source",
            Self::Evidence => "Evidence",
            Self::Payload => "Payload",
            Self::Key => "Key",
            Self::Copy => "Copy",
            Self::Backup => "Backup",
        }
    }

    fn parent_allowed(self, parent: Option<Self>) -> bool {
        matches!(
            (self, parent),
            (Self::Memory | Self::Source, None)
                | (Self::Revision, Some(Self::Memory))
                | (Self::Evidence, Some(Self::Source))
                | (Self::Payload, Some(Self::Revision | Self::Evidence))
                | (Self::Key | Self::Copy | Self::Backup, Some(Self::Payload))
        )
    }

    fn effect(self) -> Option<Effect> {
        match self {
            Self::Key => Some(Effect::KeyUnavailable),
            Self::Copy => Some(Effect::CiphertextRemoved),
            Self::Backup => Some(Effect::BackupExpired),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct Node {
    id: Id,
    kind: Kind,
    generation: i64,
    parent: Option<Id>,
}

struct Graph {
    nodes: BTreeMap<Id, Node>,
    children: BTreeMap<Id, Vec<Id>>,
    evidence: BTreeMap<Id, Vec<Id>>,
    dependents: BTreeMap<Id, Vec<Id>>,
}

impl Graph {
    fn new(nodes: &[Node], evidence: &[(Id, Id)]) -> Result<Self> {
        let mut graph = Self {
            nodes: nodes.iter().map(|node| (node.id, *node)).collect(),
            children: BTreeMap::new(),
            evidence: BTreeMap::new(),
            dependents: BTreeMap::new(),
        };
        if graph.nodes.len() != nodes.len() || graph.kind(TARGET) != Some(Kind::Memory) {
            return Err(Failure::Blocked);
        }
        for node in nodes {
            let parent = node.parent.and_then(|id| graph.kind(id));
            if !node.kind.parent_allowed(parent)
                || (node.parent.is_some() && parent.is_none())
                || node.generation <= 0
                || node.id == ZERO
            {
                return Err(Failure::Blocked);
            }
            if let Some(id) = node.parent {
                graph.children.entry(id).or_default().push(node.id);
            }
        }
        for &(rev, proof) in evidence {
            if graph.kind(rev) != Some(Kind::Revision) || graph.kind(proof) != Some(Kind::Evidence)
            {
                return Err(Failure::Blocked);
            }
            graph.evidence.entry(rev).or_default().push(proof);
            graph.dependents.entry(proof).or_default().push(rev);
        }
        for node in nodes {
            let children = graph.children(node.id);
            let count = |kind| {
                children
                    .iter()
                    .filter(|id| graph.kind(**id) == Some(kind))
                    .count()
            };
            match node.kind {
                Kind::Revision
                    if !graph.evidence.contains_key(&node.id) || count(Kind::Payload) == 0 =>
                {
                    return Err(Failure::Blocked);
                }
                Kind::Payload if count(Kind::Key) != 1 || count(Kind::Copy) == 0 => {
                    return Err(Failure::Blocked);
                }
                _ => {}
            }
        }
        Ok(graph)
    }

    fn kind(&self, id: Id) -> Option<Kind> {
        self.nodes.get(&id).map(|node| node.kind)
    }

    fn children(&self, id: Id) -> &[Id] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }

    fn node(&self, reference: ObjectRef) -> Result<Node> {
        self.nodes
            .get(&reference.id)
            .copied()
            .filter(|node| node.generation == reference.generation)
            .ok_or(Failure::Blocked)
    }

    fn closure(&self, root: Id) -> BTreeSet<Id> {
        let mut result = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(next) = pending.pop() {
            if result.insert(next) {
                pending.extend(self.children(next));
                if let Some(dependents) = self.dependents.get(&next) {
                    pending.extend(dependents);
                }
            }
        }
        result
    }

    fn required_for_revision(&self, revision: Id) -> BTreeSet<Id> {
        let mut result = BTreeSet::new();
        let mut pending = vec![revision];
        while let Some(id) = pending.pop() {
            if self.kind(id) == Some(Kind::Backup) || !result.insert(id) {
                continue;
            }
            pending.extend(self.children(id));
            match self.nodes[&id].kind {
                Kind::Evidence => result.extend(self.nodes[&id].parent),
                Kind::Revision => {
                    result.extend(self.nodes[&id].parent);
                    pending.extend(&self.evidence[&id]);
                }
                _ => {}
            }
        }
        result
    }

    fn revision_is_eligible(&self, db: &Sqlite, revision: Id) -> Result<bool> {
        Ok(self.read_binding(db, revision)?.is_some())
    }

    fn read_binding(&self, db: &Sqlite, revision: Id) -> Result<Option<[u8; 32]>> {
        if self.kind(revision) != Some(Kind::Revision) {
            return Err(Failure::Blocked);
        }
        let mut hash = Sha256::new();
        hash.update(b"YXIP/r5-read-graph/v1\0");
        hash.update(revision);
        for id in self.required_for_revision(revision) {
            let node = self.nodes[&id];
            let retired: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM r5_tombstone WHERE object=?1 AND generation=?2)",
                params![id, node.generation],
                |row| row.get(0),
            )?;
            if retired {
                return Ok(None);
            }
            hash.update(id);
            hash.update(node.generation.to_be_bytes());
        }
        Ok(Some(hash.finalize().into()))
    }
}

#[derive(Clone, Copy)]
struct ObjectRef {
    id: Id,
    generation: i64,
}

#[derive(Clone, Copy)]
enum Command {
    Grant {
        expected_epoch: i64,
    },
    Revoke,
    DisablePrincipal,
    DisableTenant,
    DisableSpace,
    Revise {
        expected: ObjectRef,
        next: ObjectRef,
    },
    InvalidateSource(ObjectRef),
    RetireKey(ObjectRef),
    RetireCopy(ObjectRef),
    Delete(ObjectRef),
}

impl Command {
    fn wire(self) -> ([u8; 57], Change) {
        let mut body = [0; 57];
        let mut object = |offset: usize, reference: ObjectRef| {
            body[offset..offset + 16].copy_from_slice(&reference.id);
            body[offset + 16..offset + 24].copy_from_slice(&reference.generation.to_be_bytes());
        };
        let (code, change) = match self {
            Self::Grant { expected_epoch } => {
                body[49..].copy_from_slice(&expected_epoch.to_be_bytes());
                (1, Change::Grant)
            }
            Self::Revoke => (2, Change::Revocation),
            Self::DisablePrincipal => (3, Change::Principal),
            Self::DisableTenant => (4, Change::Space),
            Self::DisableSpace => (5, Change::Space),
            Self::Revise { expected, next } => {
                object(1, expected);
                object(25, next);
                (6, Change::Revision)
            }
            Self::InvalidateSource(reference) => {
                object(1, reference);
                (7, Change::Source)
            }
            Self::RetireKey(reference) => {
                object(1, reference);
                (8, Change::Key)
            }
            Self::RetireCopy(reference) => {
                object(1, reference);
                (9, Change::Copy)
            }
            Self::Delete(reference) => {
                object(1, reference);
                (10, Change::Delete)
            }
        };
        body[0] = code;
        (body, change)
    }

    fn decode(body: [u8; 57]) -> Result<Self> {
        let object = |offset: usize| ObjectRef {
            id: body[offset..offset + 16].try_into().unwrap(),
            generation: i64::from_be_bytes(body[offset + 16..offset + 24].try_into().unwrap()),
        };
        let command = match body[0] {
            1 => Self::Grant {
                expected_epoch: i64::from_be_bytes(body[49..].try_into().unwrap()),
            },
            2 => Self::Revoke,
            3 => Self::DisablePrincipal,
            4 => Self::DisableTenant,
            5 => Self::DisableSpace,
            6 => Self::Revise {
                expected: object(1),
                next: object(25),
            },
            7 => Self::InvalidateSource(object(1)),
            8 => Self::RetireKey(object(1)),
            9 => Self::RetireCopy(object(1)),
            10 => Self::Delete(object(1)),
            _ => return Err(Failure::Blocked),
        };
        if command.wire().0 != body {
            return Err(Failure::Blocked);
        }
        Ok(command)
    }

    fn validate(self, graph: &Graph) -> Result<()> {
        let valid = match self {
            Self::Grant { expected_epoch } => expected_epoch > 0,
            Self::Revise { expected, next } => {
                let expected = graph.node(expected)?;
                let next = graph.node(next)?;
                expected.kind == Kind::Revision
                    && next.kind == Kind::Revision
                    && expected.parent == next.parent
                    && next.generation > expected.generation
            }
            Self::InvalidateSource(reference) => graph.node(reference)?.kind == Kind::Source,
            Self::RetireKey(reference) => graph.node(reference)?.kind == Kind::Key,
            Self::RetireCopy(reference) => graph.node(reference)?.kind == Kind::Copy,
            Self::Delete(reference) => matches!(
                graph.node(reference)?.kind,
                Kind::Memory | Kind::Source | Kind::Evidence | Kind::Payload
            ),
            _ => true,
        };
        if valid { Ok(()) } else { Err(Failure::Blocked) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Effect {
    KeyUnavailable,
    CiphertextRemoved,
    BackupExpired,
}

impl Effect {
    fn name(self) -> &'static str {
        match self {
            Self::KeyUnavailable => "KeyUnavailable",
            Self::CiphertextRemoved => "CiphertextRemoved",
            Self::BackupExpired => "BackupExpired",
        }
    }
}

struct Closure {
    graph: Graph,
    broker: Broker,
}

impl Closure {
    fn create(
        path: &Path,
        nodes: &[Node],
        evidence: &[(Id, Id)],
        current: &[(Id, Id)],
    ) -> Result<Self> {
        let graph = Graph::new(nodes, evidence)?;
        let current_map: BTreeMap<_, _> = current.iter().copied().collect();
        if current_map.len() != current.len()
            || current.len()
                != nodes
                    .iter()
                    .filter(|node| node.kind == Kind::Memory)
                    .count()
        {
            return Err(Failure::Blocked);
        }
        for &(memory, revision) in current {
            if graph.kind(memory) != Some(Kind::Memory)
                || !graph
                    .nodes
                    .get(&revision)
                    .is_some_and(|node| node.kind == Kind::Revision && node.parent == Some(memory))
            {
                return Err(Failure::Blocked);
            }
        }
        let broker = create_with(path, |tx| {
            tx.execute_batch(SCHEMA)?;
            // Deferred parent checks allow seed rows in any order; COMMIT still checks every edge.
            tx.execute_batch("PRAGMA defer_foreign_keys=ON")?;
            for node in nodes {
                tx.execute(
                    "INSERT INTO r5_object VALUES(?1,?2,?3,?4)",
                    params![node.id, node.kind.name(), node.generation, node.parent],
                )?;
            }
            for &(revision, proof) in evidence {
                tx.execute(
                    "INSERT INTO r5_evidence VALUES(?1,?2)",
                    params![revision, proof],
                )?;
            }
            for &(memory, revision) in current {
                tx.execute(
                    "INSERT INTO r5_current VALUES(?1,?2)",
                    params![memory, revision],
                )?;
            }
            tx.execute(
                "UPDATE authority SET revision=?1",
                [graph.nodes[&current_map[&TARGET]].generation],
            )?;
            tx.execute_batch(
                "CREATE TRIGGER r5_authority_update BEFORE UPDATE ON authority
                 WHEN NOT EXISTS(SELECT 1 FROM r5_active_change)
                 BEGIN SELECT RAISE(ABORT,'authority requires ordered apply'); END;",
            )?;
            for table in ["r5_object", "r5_evidence", "r5_current"] {
                freeze(tx, table, "INSERT")?;
            }
            for table in [
                "r5_object",
                "r5_evidence",
                "r5_command",
                "r5_applied",
                "r5_apply_context",
                "r5_tombstone",
                "r5_outbox",
                "r5_plan",
                "r5_observation",
                "r5_receipt",
            ] {
                freeze(tx, table, "UPDATE")?;
                freeze(tx, table, "DELETE")?;
            }
            freeze(tx, "r5_current", "DELETE")?;
            freeze(tx, "authority", "INSERT")?;
            freeze(tx, "authority", "DELETE")?;
            Ok(())
        })?;
        Ok(Self { graph, broker })
    }

    fn arm(&mut self, now: u64) -> Result<Option<Armed>> {
        let graph = &self.graph;
        self.broker.arm_bound(
            now,
            |tx| {
                let revision = current_revision(tx, TARGET)?;
                let generation: i64 =
                    tx.query_row("SELECT revision FROM authority", [], |row| row.get(0))?;
                let current = graph.nodes.get(&revision).ok_or(Failure::Blocked)?;
                if current.parent != Some(TARGET) || current.generation != generation {
                    return Err(Failure::Blocked);
                }
                graph
                    .read_binding(tx, revision)?
                    .map(Some)
                    .ok_or(Failure::Blocked)
            },
            || {},
        )
    }

    fn close(&mut self, scope: Scope, request: Id, command: Command) -> Result<i64> {
        let (body, change) = command.wire();
        let graph = &self.graph;
        self.broker.close_with(scope, |tx| {
            if request == ZERO {
                return Err(Failure::Blocked);
            }
            if let Some(event) = event_for(tx, request, body)? {
                return Ok((event, false));
            }
            command.validate(graph)?;
            let id = insert_change(tx, scope, change)?;
            tx.execute(
                "INSERT INTO r5_command VALUES(?1,?2,?3)",
                params![id, request, body],
            )?;
            Ok((id, true))
        })
    }

    fn apply(&mut self, event: i64) -> Result<()> {
        self.apply_before_commit(event, || {})
    }

    fn apply_before_commit(&mut self, event: i64, before_commit: impl FnOnce()) -> Result<()> {
        let graph = &self.graph;
        self.broker.apply_with(event, |tx, kind| {
            let body = tx.query_row(
                "SELECT body FROM r5_command WHERE event=?1",
                [event],
                |row| row.get(0),
            )?;
            let command = Command::decode(body)?;
            if command.wire().1.record().0 != kind.record().0 {
                return Err(Failure::Blocked);
            }
            command.validate(graph)?;
            tx.execute("INSERT INTO r5_apply_context VALUES(?1)", [event])?;
            mutate(tx, graph, event, command)?;
            let revision = current_revision(tx, TARGET)?;
            let allowed = graph.revision_is_eligible(tx, revision)?;
            let generation = graph.nodes[&revision].generation;
            let changed = tx.execute(
                "UPDATE authority SET epoch=epoch+1,revision=?1,source_ok=?2,
                 source=epoch+1,key_gen=epoch+1,copy_gen=epoch+1 WHERE scope=?3",
                params![generation, allowed, SCOPE.encode()],
            )?;
            if changed != 1 {
                return Err(Failure::Blocked);
            }
            tx.execute(
                "INSERT INTO r5_applied(event,epoch) SELECT ?1,epoch FROM authority",
                [event],
            )?;
            before_commit();
            Ok(())
        })
    }

    fn observe_effect(&mut self, result: cleanup::SyntheticEffect) -> Result<()> {
        if self.broker.blocked {
            return Err(Failure::Blocked);
        }
        let (owner, reference, effect) = result.identity();
        if owner != self.broker.core.incarnation {
            return Err(Failure::Conflict);
        }
        self.broker.transact(|tx| {
            let kind = effect.name();
            let matches: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM r5_outbox w JOIN r5_plan p USING(object,generation)
                 JOIN security_event e ON e.id=p.event WHERE e.state='Applied'
                 AND w.object=?1 AND w.generation=?2 AND w.action=?3)",
                params![reference.id, reference.generation, kind],
                |row| row.get(0),
            )?;
            if !matches {
                return Err(Failure::Conflict);
            }
            // This evidence represents the result of a synthetic adapter, not a cleanup permission.
            tx.execute(
                "INSERT INTO r5_observation VALUES(?1,?2,?3)
                        ON CONFLICT(object,generation) DO NOTHING",
                params![reference.id, reference.generation, kind],
            )?;
            Ok(())
        })
    }

    fn settle_effect(&mut self, reference: ObjectRef) -> Result<()> {
        if self.broker.blocked {
            return Err(Failure::Blocked);
        }
        self.broker.transact(|tx| {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM r5_observation WHERE object=?1 AND generation=?2)",
                params![reference.id, reference.generation], |row| row.get(0),
            )?;
            if !exists { return Err(Failure::Conflict); }
            tx.execute(
                "INSERT INTO r5_receipt SELECT * FROM r5_observation WHERE object=?1 AND generation=?2
                 ON CONFLICT(object,generation) DO NOTHING", params![reference.id, reference.generation],
            )?;
            Ok(())
        })
    }
}

fn freeze(tx: &Transaction<'_>, table: &str, operation: &str) -> Result<()> {
    tx.execute_batch(&format!(
        "CREATE TRIGGER freeze_{table}_{operation} BEFORE {operation} ON {table}
         BEGIN SELECT RAISE(ABORT,'immutable security graph or ledger'); END;"
    ))?;
    Ok(())
}

fn event_for(db: &Sqlite, request: Id, body: [u8; 57]) -> Result<Option<i64>> {
    let prior: Option<(i64, [u8; 57])> = db
        .query_row(
            "SELECT event,body FROM r5_command WHERE request=?1",
            [request],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match prior {
        Some((event, recorded)) if recorded == body => Ok(Some(event)),
        Some(_) => Err(Failure::Conflict),
        None => Ok(None),
    }
}

fn current_revision(db: &Sqlite, memory: Id) -> Result<Id> {
    Ok(db.query_row(
        "SELECT revision FROM r5_current WHERE memory=?1",
        [memory],
        |row| row.get(0),
    )?)
}

fn mutate(tx: &Transaction<'_>, graph: &Graph, event: i64, command: Command) -> Result<()> {
    match command {
        Command::Grant { expected_epoch } => {
            if tx.execute(
                "UPDATE authority SET grant_ok=1 WHERE epoch=?1 AND principal_ok=1 AND space_ok=1",
                [expected_epoch],
            )? != 1
            {
                return Err(Failure::Blocked);
            }
        }
        Command::Revoke => {
            tx.execute("UPDATE authority SET grant_ok=0", [])?;
        }
        Command::DisablePrincipal => {
            tx.execute("UPDATE authority SET principal_ok=0", [])?;
        }
        Command::DisableTenant | Command::DisableSpace => {
            // One tenant/space per experimental realm. Neither can be re-enabled here.
            tx.execute("UPDATE authority SET space_ok=0", [])?;
        }
        Command::Revise { expected, next } => {
            let memory = graph.node(next)?.parent.ok_or(Failure::Blocked)?;
            if !graph.revision_is_eligible(tx, next.id)?
                || tx.execute(
                    "UPDATE r5_current SET revision=?1 WHERE memory=?2 AND revision=?3",
                    params![next.id, memory, expected.id],
                )? != 1
            {
                return Err(Failure::Blocked);
            }
        }
        Command::Delete(root) => {
            for id in graph.closure(root.id) {
                let node = graph.nodes[&id];
                retire(tx, event, node, node.kind.effect())?;
            }
        }
        Command::InvalidateSource(root) => {
            for id in graph.closure(root.id) {
                retire(tx, event, graph.nodes[&id], None)?;
            }
        }
        Command::RetireKey(root) => {
            retire(tx, event, graph.node(root)?, Some(Effect::KeyUnavailable))?
        }
        Command::RetireCopy(root) => retire(tx, event, graph.node(root)?, None)?,
    }
    Ok(())
}

fn retire(tx: &Transaction<'_>, event: i64, node: Node, effect: Option<Effect>) -> Result<()> {
    tx.execute(
        "INSERT INTO r5_tombstone VALUES(?1,?2,?3) ON CONFLICT(object,generation) DO NOTHING",
        params![node.id, node.generation, event],
    )?;
    if let Some(effect) = effect {
        tx.execute(
            "INSERT INTO r5_outbox VALUES(?1,?2,?3) ON CONFLICT(object,generation) DO NOTHING",
            params![node.id, node.generation, effect.name()],
        )?;
        tx.execute(
            "INSERT INTO r5_plan VALUES(?1,?2,?3)",
            params![event, node.id, node.generation],
        )?;
    }
    Ok(())
}

/// The executor has no Windows, key vault, filesystem or production effect capability.
mod cleanup {
    use super::{Effect, Id, ObjectRef};

    pub(super) struct SyntheticEffect {
        owner: Id,
        object: ObjectRef,
        effect: Effect,
    }

    impl SyntheticEffect {
        pub(super) fn identity(&self) -> (Id, ObjectRef, Effect) {
            (self.owner, self.object, self.effect)
        }
    }

    pub(super) fn observe(owner: Id, object: ObjectRef, effect: Effect) -> SyntheticEffect {
        SyntheticEffect {
            owner,
            object,
            effect,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Progress {
    state: String,
    pending: i64,
    observed_unsettled: i64,
    keys: i64,
    copies: i64,
    backups: i64,
}

fn progress(db: &Sqlite, event: i64) -> Result<Progress> {
    Ok(db.query_row(
        "SELECT e.state,
           count(w.object) FILTER(WHERE r.object IS NULL),
           count(o.object) FILTER(WHERE r.object IS NULL),
           count(r.object) FILTER(WHERE r.action='KeyUnavailable'),
           count(r.object) FILTER(WHERE r.action='CiphertextRemoved'),
           count(r.object) FILTER(WHERE r.action='BackupExpired')
         FROM security_event e LEFT JOIN r5_plan p ON p.event=e.id
         LEFT JOIN r5_outbox w USING(object,generation)
         LEFT JOIN r5_observation o USING(object,generation)
         LEFT JOIN r5_receipt r USING(object,generation)
         WHERE e.id=?1 GROUP BY e.id",
        [event],
        |row| {
            Ok(Progress {
                state: row.get(0)?,
                pending: row.get(1)?,
                observed_unsettled: row.get(2)?,
                keys: row.get(3)?,
                copies: row.get(4)?,
                backups: row.get(5)?,
            })
        },
    )?)
}

struct Recovery {
    blocked: RecoveryBlocked,
}

impl Recovery {
    fn inspect(path: &Path) -> Result<Self> {
        let blocked = RecoveryBlocked::inspect_profile(path, APPLICATION_ID)?;
        let inconsistent: bool = blocked.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM security_event e LEFT JOIN r5_command c ON e.id=c.event
                 LEFT JOIN r5_applied a ON e.id=a.event
                 WHERE e.scope<>?1 OR c.event IS NULL OR ((e.state='Applied')<>(a.event IS NOT NULL)))
             OR (SELECT epoch FROM authority) IS NOT (SELECT coalesce(max(epoch),7) FROM r5_applied)
             OR EXISTS(SELECT 1 FROM r5_outbox w LEFT JOIN r5_plan p USING(object,generation)
                 WHERE p.event IS NULL)
             OR EXISTS(SELECT 1 FROM r5_current c JOIN r5_object o ON o.id=c.revision
                 WHERE o.kind<>'Revision' OR o.parent IS NOT c.memory)",
            [SCOPE.encode()],
            |row| row.get(0),
        )?;
        if inconsistent {
            return Err(Failure::Blocked);
        }
        Ok(Self { blocked })
    }

    fn progress(&self, event: i64) -> Result<Progress> {
        progress(&self.blocked.db, event)
    }

    fn reconcile(&self, scope: Scope, request: Id, command: Command) -> Result<Option<Progress>> {
        if scope != SCOPE || request == ZERO {
            return Err(Failure::Blocked);
        }
        event_for(&self.blocked.db, request, command.wire().0)?
            .map(|event| self.progress(event))
            .transpose()
    }
}

mod cases;

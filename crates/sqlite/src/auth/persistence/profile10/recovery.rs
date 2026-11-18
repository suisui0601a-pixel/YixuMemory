use super::*;

#[derive(Debug)]
pub(super) struct Reconciliation {
    pub(super) blocked: bool,
    pub(super) committed: usize,
    pub(super) states: Vec<(Id, String)>,
}

/// Trusted parent retains the actual owned Child HANDLE and consumes its wait result.
/// Its incarnation must be read from the child's startup acknowledgement in the owned test root.
pub(super) struct ExitProof {
    incarnation: Id,
    installation: Installation,
    nonce: Id,
}

/// Only this trusted supervisor can construct an exit proof.
/// Startup acknowledgement arrives on the exact Child's inherited stdout, with a fresh launch nonce.
pub(super) struct OwnerProcess {
    child: std::process::Child,
    incarnation: Id,
    installation: Installation,
    nonce: Id,
    drainer: Option<std::thread::JoinHandle<()>>,
}
impl OwnerProcess {
    pub(super) fn launch(
        root: &Path,
        installation: Installation,
        point: &str,
        seed: &str,
    ) -> Result<Self> {
        Self::launch_next(root, installation, point, seed, None)
    }

    pub(super) fn launch_next(
        root: &Path,
        installation: Installation,
        point: &str,
        seed: &str,
        previous: Option<ExitProof>,
    ) -> Result<Self> {
        use std::io::{BufRead, Write};
        let nonce = fresh()?;
        let mut log = File::create(
            root.join("control")
                .join(format!("child-stdout-{}.log", filename(nonce))),
        )
        .map_err(|_| Error::ResourceUnavailable)?;
        let stderr = File::create(
            root.join("control")
                .join(format!("child-stderr-{}.log", filename(nonce))),
        )
        .map_err(|_| Error::ResourceUnavailable)?;
        let mut command = std::process::Command::new(
            std::env::current_exe().map_err(|_| Error::ResourceUnavailable)?,
        );
        if let Some(proof) = previous {
            command.env("YIXU_P10_PREV", filename(proof.incarnation));
            command.env("YIXU_P10_PREV_NONCE", filename(proof.nonce));
            if proof.installation.id != installation.id
                || proof.installation.realm != installation.realm
            {
                return Err(Error::Integrity);
            }
        }
        let mut child = command
            .args([
                "--exact",
                "auth::persistence::profile10::tests::owned_process_entry",
                "--nocapture",
            ])
            .env("YIXU_P10_CHILD", root)
            .env("YIXU_P10_POINT", point)
            .env("YIXU_P10_SEED", seed)
            .env("YIXU_P10_NONCE", filename(nonce))
            .env("TMP", root.join("temp"))
            .env("TEMP", root.join("temp"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::from(stderr))
            .spawn()
            .map_err(|_| Error::ResourceUnavailable)?;
        let stdout = child.stdout.take().ok_or(Error::ResourceUnavailable)?;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let drainer = std::thread::spawn(move || {
            let mut acknowledged = false;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let _ = writeln!(log, "{line}");
                if !acknowledged && let Some(ack) = line.strip_prefix("P10_OWNER_START ") {
                    let ids: Option<Vec<Id>> = ack
                        .split_whitespace()
                        .map(|s| s.parse::<yixu_domain::Id>().ok().map(raw))
                        .collect();
                    let _ = sender.send(ids);
                    acknowledged = true;
                }
            }
        });
        let start = receiver.recv_timeout(std::time::Duration::from_secs(30));
        let valid = start.ok().flatten().filter(|ids| {
            ids.len() == 4
                && ids[0] == installation.id
                && ids[1] == installation.realm
                && ids[3] == nonce
        });
        let Some(ids) = valid else {
            let _ = child.kill();
            let _ = child.wait();
            let _ = drainer.join();
            return Err(Error::CapabilityUnavailable);
        };
        Ok(Self {
            child,
            incarnation: ids[2],
            installation,
            nonce,
            drainer: Some(drainer),
        })
    }
    pub(super) fn incarnation(&self) -> Id {
        self.incarnation
    }
    pub(super) fn has_exited(&mut self) -> Result<bool> {
        Ok(self
            .child
            .try_wait()
            .map_err(|_| Error::ResourceUnavailable)?
            .is_some())
    }
    pub(super) fn wait_clean(mut self) -> Result<ExitProof> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !self.has_exited()? && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if std::time::Instant::now() >= deadline {
            let _ = self.child.kill();
        }
        let success = self
            .child
            .wait()
            .map_err(|_| Error::ResourceUnavailable)?
            .success();
        if let Some(drainer) = self.drainer.take() {
            let _ = drainer.join();
        }
        if !success {
            return Err(Error::CapabilityUnavailable);
        }
        Ok(ExitProof {
            incarnation: self.incarnation,
            installation: self.installation,
            nonce: self.nonce,
        })
    }
    pub(super) fn kill(mut self) -> Result<()> {
        let _ = self.child.kill();
        self.child.wait().map_err(|_| Error::ResourceUnavailable)?;
        if let Some(drainer) = self.drainer.take() {
            let _ = drainer.join();
        }
        Ok(())
    }
}
impl Drop for OwnerProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(drainer) = self.drainer.take() {
            let _ = drainer.join();
        }
    }
}

fn ledger_hash(db: &Connection) -> Result<[u8; 32]> {
    use rusqlite::types::ValueRef;
    let mut hash = Sha256::new();
    hash.update(b"YIXU/P10/CLEAN-STOP-LEDGER/v1\0");
    for table in [
        "runtime",
        "authority",
        "source",
        "evidence",
        "security_event",
        "security_receipt",
        "r5_tombstone",
        "write_intent",
        "publication",
        "memory",
        "revision",
        "payload",
        "local_key",
        "revision_evidence",
        "receipt",
        "outbox",
        "transition",
    ] {
        hash.update(table.as_bytes());
        let query = if table == "runtime" {
            "SELECT installation,realm,incarnation,owner_nonce,store_identity,schema_hash,epoch,knowledge_seq,blocked FROM runtime".to_owned()
        } else {
            format!("SELECT * FROM {table} ORDER BY rowid")
        };
        let mut statement = sql(db.prepare(&query))?;
        let columns = statement.column_count();
        let mut rows = sql(statement.query([]))?;
        while let Some(row) = sql(rows.next())? {
            hash.update(b"row");
            for col in 0..columns {
                match sql(row.get_ref(col))? {
                    ValueRef::Null => hash.update([0]),
                    ValueRef::Integer(i) => {
                        hash.update([1]);
                        hash.update(i.to_be_bytes());
                    }
                    ValueRef::Blob(b) | ValueRef::Text(b) => {
                        hash.update([2]);
                        hash.update((b.len() as u64).to_be_bytes());
                        hash.update(b);
                    }
                    ValueRef::Real(_) => return Err(Error::Integrity),
                }
            }
        }
    }
    Ok(hash.finalize().into())
}

impl Engine {
    pub(super) fn open_existing(root: &Path, installation: Installation) -> Result<Self> {
        let anchor = winfs::Anchor::open(root).map_err(|_| Error::CapabilityUnavailable)?;
        let lock = anchor.broker_lock().map_err(|_| Error::Busy)?;
        check_layout(&anchor, false)?;
        let files = winfs::Anchor::open(&root.join("payloads"))
            .map_err(|_| Error::CapabilityUnavailable)?;
        let vault = LocalVault::new(root.join("keys"))?;
        let db = sql(Connection::open_with_flags(
            root.join("content.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ))?;
        configure(&db)?;
        attest(&db)?;
        let (id, realm, incarnation, hash, store): (Id, Id, Id, Vec<u8>, Vec<u8>) = sql(db
            .query_row(
                "SELECT installation,realm,incarnation,schema_hash,store_identity FROM runtime",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            ))?;
        if id != installation.id
            || realm != installation.realm
            || hash != digest(DDL.as_bytes())
            || store != storage_identity(&anchor, &files, &vault)?
        {
            return Err(Error::Integrity);
        }
        Ok(Self {
            db,
            installation,
            incarnation,
            files,
            vault,
            _lock: lock,
            before_commit: None,
            recovery_only: true,
        })
    }

    pub(super) fn reconcile(&self, _fixture_time: i64) -> Result<Reconciliation> {
        let now = trusted_now()?;
        attest(&self.db)?;
        let mut result = Reconciliation {
            blocked: sql(self
                .db
                .query_row("SELECT blocked FROM runtime", [], |r| r.get(0)))?
                || sql(gates::closing(&self.db))?,
            committed: 0,
            states: Vec::new(),
        };
        let clean: bool = sql(self
            .db
            .query_row("SELECT clean FROM runtime", [], |r| r.get(0)))?;
        if self.recovery_only && !clean {
            result.blocked = true;
        }
        let bad:bool=sql(self.db.query_row(
            "SELECT
              EXISTS(SELECT 1 FROM write_intent i WHERE (i.state='Committed') <>
                EXISTS(SELECT 1 FROM receipt WHERE operation=i.operation))
              OR EXISTS(SELECT 1 FROM receipt r WHERE NOT EXISTS(
                 SELECT 1 FROM outbox o WHERE o.operation=r.operation AND o.knowledge_seq=r.knowledge_seq))
              OR EXISTS(SELECT 1 FROM receipt r JOIN security_event e ON e.id=r.event
                  WHERE e.kind<>'Write' OR e.state<>'Applied' OR e.scope<>r.scope)
              OR EXISTS(SELECT 1 FROM payload p JOIN local_key k ON k.scope=p.scope
                 AND k.payload=p.id AND k.id=p.key_id WHERE k.revoked<>0 OR k.generation<>p.generation)
              OR (SELECT knowledge_seq FROM runtime)<>(SELECT count(*) FROM receipt)
              OR (SELECT epoch FROM runtime)<>7+(SELECT count(*) FROM security_receipt)
              OR EXISTS(SELECT 1 FROM authority WHERE epoch<>(SELECT epoch FROM runtime))",
            [],|r|r.get(0),
        ))?;
        result.blocked |= bad;
        let rows: Vec<(Vec<u8>, Id, Id)> = {
            let mut q = sql(self
                .db
                .prepare("SELECT scope,principal,request FROM write_intent ORDER BY operation"))?;
            sql(sql(q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))?.collect())?
        };
        let epoch: i64 = sql(self
            .db
            .query_row("SELECT epoch FROM runtime", [], |r| r.get(0)))?;
        let mut payload_names = std::collections::BTreeSet::new();
        let mut key_names = std::collections::BTreeSet::new();
        for (scope, principal, request) in rows {
            if scope.len() != 48 {
                return Err(Error::Integrity);
            }
            let identity = Identity {
                scope: SecurityScope {
                    realm: scope[..16].try_into().map_err(|_| Error::Integrity)?,
                    tenant: scope[16..32].try_into().map_err(|_| Error::Integrity)?,
                    space: scope[32..].try_into().map_err(|_| Error::Integrity)?,
                },
                principal,
            };
            if identity.scope.realm != self.installation.realm {
                return Err(Error::Integrity);
            }
            let (plan, state) =
                Self::plan_row(&self.db, identity, request)?.ok_or(Error::Integrity)?;
            payload_names.insert(filename(plan.payload));
            key_names.insert(filename(plan.key));
            let phase = match state.as_str() {
                "Pending" => "PendingNotStarted",
                "Staged" => "IntentEstablished",
                "Sealing" => "PublicationUnknown",
                "Published" => "PublicationConfirmed",
                "Unknown" => "OutputUnknown",
                "Committed" if self.recovery_only && !clean => "CommittedReceiptResponseUnknown",
                "Committed" => "CommittedReceipt",
                _ => return Err(Error::Integrity),
            };
            if state != "Committed" {
                result.blocked = true;
                result.states.push((plan.operation, phase.to_owned()));
                continue;
            }
            // Cryptographic verification never confers a content-read permission.
            let verified = self.verify_files(&plan);
            let eligible = plan
                .expected_dispatch(&self.db, self.installation)
                .is_ok_and(|binding| binding == plan.dispatch)
                && sql(gates::authorized_snapshot(
                    &self.db,
                    &scope,
                    &principal,
                    &plan.request.memory,
                    epoch,
                    now,
                ))?
                .is_some();
            let evidence_ok:bool=sql(self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM revision_evidence re JOIN evidence e
                  ON e.scope=re.scope AND e.id=re.evidence AND e.generation=re.generation
                  JOIN source s ON s.scope=e.scope AND s.id=e.source AND s.generation=e.source_generation
                  JOIN memory m ON m.scope=re.scope AND m.id=re.memory
                  WHERE re.scope=?1 AND re.memory=?2 AND re.revision=?3 AND re.evidence=?4
                   AND re.generation=?5 AND e.valid=1 AND s.valid=1 AND m.current_revision=re.revision)",
                params![scope,plan.request.memory,plan.request.revision,plan.request.evidence,
                    plan.request.evidence_generation],|r|r.get(0),
            ))?;
            match verified {
                Ok((plain, _)) if eligible && evidence_ok => {
                    drop(plain);
                    result.committed += 1;
                    result.states.push((plan.operation, phase.to_owned()));
                }
                Ok(_) => {
                    // Fully accounted historical data can be retained while current Grant denies it.
                    // B1 has no read interface. Future release must check current eligibility again.
                    result.committed += 1;
                    result
                        .states
                        .push((plan.operation, "SecurityStateDenied".into()));
                }
                Err(error) => {
                    result.blocked = true;
                    let status = match error {
                        Error::ResourceUnavailable => "CiphertextMissingOrUnavailable",
                        Error::CapabilityUnavailable => "KeyUnavailable",
                        _ => "IntegrityRejected",
                    };
                    result.states.push((plan.operation, status.into()));
                }
            }
        }
        let unknown = self
            .files
            .names()
            .map_err(|_| Error::Integrity)?
            .iter()
            .any(|name| !payload_names.contains(name))
            || self
                .vault
                .anchor
                .names()
                .map_err(|_| Error::Integrity)?
                .iter()
                .any(|name| !key_names.contains(name));
        if unknown {
            result.blocked = true;
            result
                .states
                .push(([0; 16], "UnregisteredStorageObjects".into()));
        }
        Ok(result)
    }

    pub(super) fn clean_stop(&mut self, now: i64) -> Result<()> {
        if self.reconcile(now)?.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        // Single actor and held W1 lock: no content operation can begin after this flag.
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        if sql(gates::closing(&tx))? {
            return Err(Error::CapabilityUnavailable);
        }
        let hash = ledger_hash(&tx)?;
        sql(tx.execute(
            "INSERT INTO stop_receipt
             SELECT incarnation,installation,realm,owner_nonce,epoch,knowledge_seq,schema_hash,?1
             FROM runtime WHERE clean=0 AND blocked=0",
            [hash],
        ))?;
        sql(tx.execute("UPDATE runtime SET clean=1 WHERE blocked=0", []))?;
        sql(tx.commit())
    }
    pub(super) fn resume(
        root: &Path,
        installation: Installation,
        proof: ExitProof,
        now: i64,
    ) -> Result<Self> {
        let engine = Self::open_existing(root, installation)?;
        if proof.installation.id != installation.id
            || proof.installation.realm != installation.realm
            || proof.incarnation != engine.incarnation
            || engine.reconcile(now)?.blocked
        {
            return Err(Error::CapabilityUnavailable);
        }
        Self::resume_checked(engine, installation, proof, fresh()?)
    }

    pub(super) fn resume_supervised(
        root: &Path,
        installation: Installation,
        nonce: Id,
    ) -> Result<Self> {
        let parse = |name| -> Result<Id> {
            std::env::var(name)
                .map_err(|_| Error::CapabilityUnavailable)?
                .parse::<yixu_domain::Id>()
                .map(raw)
                .map_err(|_| Error::Integrity)
        };
        let proof = ExitProof {
            incarnation: parse("YIXU_P10_PREV")?,
            nonce: parse("YIXU_P10_PREV_NONCE")?,
            installation,
        };
        let engine = Self::open_existing(root, installation)?;
        if proof.incarnation != engine.incarnation || engine.reconcile(0)?.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        Self::resume_checked(engine, installation, proof, nonce)
    }

    fn resume_checked(
        mut engine: Self,
        _installation: Installation,
        proof: ExitProof,
        nonce: Id,
    ) -> Result<Self> {
        let next = fresh()?;
        let tx = sql(engine
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let hash = ledger_hash(&tx)?;
        let clean: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runtime r JOIN stop_receipt s ON s.incarnation=r.incarnation
              WHERE r.clean=1 AND r.blocked=0 AND r.incarnation=?1 AND r.owner_nonce=?2
              AND s.installation=r.installation AND s.realm=r.realm AND s.owner_nonce=r.owner_nonce
              AND s.epoch=r.epoch AND s.knowledge_seq=r.knowledge_seq AND s.schema_hash=r.schema_hash
              AND s.ledger_hash=?3 AND NOT EXISTS(
                SELECT 1 FROM resume_receipt WHERE previous_incarnation=r.incarnation))",
            params![proof.incarnation,proof.nonce,hash],
            |r| r.get(0),
        ))?;
        if !clean || sql(gates::closing(&tx))? {
            return Err(Error::CapabilityUnavailable);
        }
        sql(tx.execute(
            "INSERT INTO resume_receipt SELECT incarnation,?1,epoch,?2 FROM runtime",
            params![next, hash],
        ))?;
        sql(tx.execute(
            "UPDATE runtime SET incarnation=?1,owner_nonce=?2,clean=0",
            params![next, nonce],
        ))?;
        sql(tx.commit())?;
        engine.incarnation = next;
        engine.recovery_only = false;
        Ok(engine)
    }
}

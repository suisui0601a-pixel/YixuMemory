# 领域、键与约束

本文件定义目标关系模型；当前未提供生产 DDL。所有 ID 为服务端随机 UUID 字符串，不编码个人信息。除 Tenant 外所有 PK 均为 `(tenant_id,id)`；所有 FK 显式携带 tenant_id，禁止仅按 UUID 引用。创建时刻由服务端产生 UTC RFC3339；有效时间可由来源声明，必须区分。

| 实体 | 必备字段（除公共 tenant_id/id） | 关系、唯一性与约束 |
|---|---|---|
| Tenant | status, auth_epoch, knowledge_seq, created_at | 隔离根；停用后拒绝用例 |
| Principal | kind(user/service), external_subject, status | unique(tenant,issuer,subject)；认证映射由服务端管理 |
| Group | kind(team/project/role), name, status | role 为记忆所属角色空间，不等同授权角色 |
| Membership | principal_id, group_id, role, valid_until, version | 同租户 FK；unique(principal,group)；撤权递增 auth_epoch |
| Space | scope(personal/role/team/project), owner_id, policy_version, security_floor | owner_id 指向 Principal 或 Group，scope 与 owner kind 必须匹配 |
| Grant | principal_id/group_id, space_id, capability, expires_at | 恰有一个主体；租户管理员不自动获内容读取权 |
| Payload | space_id, purpose, declared_classification, effective_classification, protection_policy_epoch, encrypted_ref, key_ref, encryption_profile, seal_context_id, generation, state | 全生产内容先加密；声明不可信，未知 sensitive；空间内独立可擦除 key，跨 Space 不共享；只有托管引用，擦除不复用 key |
| Event | space_id, kind, payload_id, occurred_at, recorded_at, knowledge_seq, actor_id, schema_version | 持久逻辑事件不可改；可空 payload_id；同空间 Payload 引用 |
| Memory | space_id, current_revision_id, version, state, created_at | current revision 必属本 memory；version 单调递增 CAS |
| Revision | memory_id, number, parent_revision_id, assertion_kind, content_payload_id, valid_from, valid_to, recorded_at, knowledge_seq, evidence_ids | unique(memory,number)；parent 同 memory 且 number-1；首版无 parent；至少一条 Evidence；当前指针与插入原子提交 |
| Evidence | event_id, source_kind, source_locator, payload_id, captured_at | event/payload 同空间；locator 为可擦除 payload 内定位信息，不把原文或 URL 写入信封；Evidence 无自由明文 |
| RevisionEvidence | revision_id, evidence_id, relation(supports/contradicts/context) | 复合 PK；两端同 tenant、space；证据关联与 Revision 同事务 |
| Conflict | left_revision_id, right_revision_id, status, resolution_revision_id | 两端不同、同空间；允许跨 memory；resolution 必须引用双方证据或解释性事件 |
| IdempotencyRecord | actor_id, operation, key, request_digest, result_ref, expires_at | unique(tenant,actor,operation,key)；digest 覆盖主体/目标/规范化请求；不存原文响应 |
| OutboxJob | kind, aggregate_id, generation, status, lease_until, lease_token, attempts, next_run_at | unique(tenant,kind,aggregate,generation)；只携带 ID；有界重试与 fencing token |
| DeletionRequest | space_id, operation, requested_scope, effective_scope, target_type/id, status, graph_cut_off, closure_ref/version, policy_epoch, generation, requested_by, coverage, receipt_set_ref | p0-sc-1；仅接受 Space 显式图操作，广义请求执行前拒绝；completed 需全部受控副本/备份/key 回执，重试不放行 |
| ErasureReceipt | deletion_id, store, copy_id, generation, closure_version, status, completed_at | unique(deletion,store,copy_id,generation)；必须覆盖每个登记副本，不能只给存储一个假总回执；无原文/稳定内容摘要/定位 |
| AuditEntry | action, outcome, policy_version, opaque_target, recorded_at | 最小不可变信封；用户映射可擦除；访问/导出也审计 |
| IndexGeneration | space_id, version, cursor, state, auth_epoch | 构建/切换/退役；索引条目带 revision_id 和删除 generation |
| MigrationRecord | version, checksum, phase, applied_at | 各 DB schema 全局表；checksum 不可偷偷变更 |
| BackupManifest | backup_id, schema_version, deletion_watermark, restore_status | 运维控制面；不得含内容；恢复前应用删除清单 |

跨租户引用、跨空间证据、环状 parent、丢失来源、空 Evidence 数组、断裂 current 指针、number 不连续都必须拒绝。JSON Schema 只验证形状，不验证 FK、鉴权、时间先后、CAS、完整来源图；由用例与数据库约束及集成测试共同承担。

事实分类：Event.kind = raw_event / external_document / user_statement / system_action；Evidence.source_kind = raw_event / external_document / user_statement / model_output / fictional_setting。Revision.assertion_kind = extracted_fact / model_inference / fictional_setting / user_assertion。模型输出不能自动标成 observed fact；推断必须有原始输入 Evidence、模型产物 Evidence 和模型/提示版本的可擦除 Provenance Payload。虚构设定只能在显式 fiction 查询域可见。

时间采用双时间：valid_from/valid_to 是半开业务有效区间，recorded_at 为事务接纳时间，knowledge_seq 为已提交知识顺序，occurred_at 是来源事件时刻。过去事实修订追加新版，不重写旧版；as_of 使用服务端返回的知识提交 token，按 knowledge_seq 选择对应快照内的 revision，再按 valid_at 过滤，不把未来修订泄漏到过去。无 valid_to 表示无界，不表示永久正确。

修订保存历史但删除例外：所有版本内容可被法定擦除，遗留结构不可重新构造原文。归档/逻辑删除不等于擦除。

## P0-SC 补充实体及边界

| 实体 | 键/关系及不变量 |
|---|---|
| DeletionClosure | tenant/id，关联 deletion_id/Space/graph_cut_off/version；保存显式成员与受控副本清单，冻结后不可缩窄；内容只用 opaque IDs |
| RevisionInvalidation | tenant/id，FK revision_id/deletion_id/Space/generation，unique(revision,deletion,generation)；追加且不可取消，所有读取应用当前记录 |
| DerivedLink | tenant/id，Space 内 dependent/source 的强类型复合引用；unique(dependent,source,relation)；与派生对象同事务登记，禁止漏边及跨 Space |
| PayloadOwner | tenant/payload_id/owner_kind/owner_id 复合 PK，同 Space；完整反向引用，Revision 正文须独立删除单元；不能把引用数猜测当完整登记 |
| ProtectionBinding | tenant/payload_id，Space 同 FK，effective_classification/policy_epoch/generation/lineage_ref；保护只可上调，更新投影与追加保护事件同事务；不改逻辑原文 |
| StagingIntent | tenant/id，Space/actor/purpose/generation/TTL/key 意图/cleanup journal/状态；密文创建前持久登记，失败立即不可水合，补偿验证前不删登记 |

Event/Evidence/Revision 有效分类经关联 Payload 与完整来源图 join，不从 assertion_kind/source_kind 或旧快照推定。Memory.source_blocked 表示来源擦除使旧 Revision 永久不可用；新来源只能合法追加修订，不能恢复原事实。独立来源不由 Memory 隐式拥有。见 [删除范围](DELETION_SCOPE.md)、[Payload 保护](PAYLOAD_PROTECTION.md) 与 ADR-010/011；旧无范围/保护记录不可默认补安全标志迁移。

## R1 图投影类型校验补充（2026-10-29）

删除 oracle 的端点/字段矩阵见 [DELETION_SCOPE](DELETION_SCOPE.md) 与 contracts/security/graph-relations.json。Evidence→Event、Revision→Evidence、Revision→Memory 不可因 UUID 存在而互换；Memory 版本必须是正整数而非 bool。当前 derived_from 投影表示 Event/Evidence/Revision 内容间的派生（含模型产物 Evidence），Memory 聚合引用需落到具体 Revision；来源捕获、聚合归属与受控副本仍各走其明确关系。Payload owner 保留 Event/Evidence/Revision/Memory 的既有归属边界。全图检查包括不在本次闭包路径上的行，未知类型/关系拒绝而非忽略。这里补足前版未展开的强类型投影约束，不改 R14 分类/加密接口，也不把合成 ID/布尔观察当真实 FK 或快照证明。

# 领域、键与约束

本文件定义目标关系模型；当前未提供生产 DDL。所有 ID 为服务端随机 UUID 字符串，不编码个人信息。除 Tenant 外所有 PK 均为 `(tenant_id,id)`；所有 FK 显式携带 tenant_id，禁止仅按 UUID 引用。创建时刻由服务端产生 UTC RFC3339；有效时间可由来源声明，必须区分。

| 实体 | 必备字段（除公共 tenant_id/id） | 关系、唯一性与约束 |
|---|---|---|
| Tenant | status, auth_epoch, knowledge_seq, created_at | 隔离根；停用后拒绝用例 |
| Principal | kind(user/service), external_subject, status | unique(tenant,issuer,subject)；认证映射由服务端管理 |
| Group | kind(team/project/role), name, status | role 为记忆所属角色空间，不等同授权角色 |
| Membership | principal_id, group_id, role, valid_until, version | 同租户 FK；unique(principal,group)；撤权递增 auth_epoch |
| Space | scope(personal/role/team/project), owner_id, policy_version | owner_id 指向 Principal 或 Group，scope 与 owner kind 必须匹配 |
| Grant | principal_id/group_id, space_id, capability, expires_at | 恰有一个主体；租户管理员不自动获内容读取权 |
| Payload | classification, encrypted_ref, key_ref, state, erased_at | 外部引用仅允许托管存储；无任意 URL 拉取；擦除后不复用 key |
| Event | space_id, kind, payload_id, occurred_at, recorded_at, knowledge_seq, actor_id, schema_version | 持久逻辑事件不可改；可空 payload_id；同空间 Payload 引用 |
| Memory | space_id, current_revision_id, version, state, created_at | current revision 必属本 memory；version 单调递增 CAS |
| Revision | memory_id, number, parent_revision_id, assertion_kind, content_payload_id, valid_from, valid_to, recorded_at, knowledge_seq, evidence_ids | unique(memory,number)；parent 同 memory 且 number-1；首版无 parent；至少一条 Evidence；当前指针与插入原子提交 |
| Evidence | event_id, source_kind, source_locator, payload_id, captured_at | event/payload 同空间；locator 为可擦除 payload 内定位信息，不把原文或 URL 写入信封；Evidence 无自由明文 |
| RevisionEvidence | revision_id, evidence_id, relation(supports/contradicts/context) | 复合 PK；两端同 tenant、space；证据关联与 Revision 同事务 |
| Conflict | left_revision_id, right_revision_id, status, resolution_revision_id | 两端不同、同空间；允许跨 memory；resolution 必须引用双方证据或解释性事件 |
| IdempotencyRecord | actor_id, operation, key, request_digest, result_ref, expires_at | unique(tenant,actor,operation,key)；digest 覆盖主体/目标/规范化请求；不存原文响应 |
| OutboxJob | kind, aggregate_id, generation, status, lease_until, lease_token, attempts, next_run_at | unique(tenant,kind,aggregate,generation)；只携带 ID；有界重试与 fencing token |
| DeletionRequest | space_id, target_type, target_id, status, cut_off, generation, requested_by | 目标与快照闭包；重试不可重新放行读；purged 前验证各存储回执 |
| ErasureReceipt | deletion_id, store, generation, status, completed_at | unique(deletion,store,generation)；不含内容、可反查明文的摘要或原始定位 |
| AuditEntry | action, outcome, policy_version, opaque_target, recorded_at | 最小不可变信封；用户映射可擦除；访问/导出也审计 |
| IndexGeneration | space_id, version, cursor, state, auth_epoch | 构建/切换/退役；索引条目带 revision_id 和删除 generation |
| MigrationRecord | version, checksum, phase, applied_at | 各 DB schema 全局表；checksum 不可偷偷变更 |
| BackupManifest | backup_id, schema_version, deletion_watermark, restore_status | 运维控制面；不得含内容；恢复前应用删除清单 |

跨租户引用、跨空间证据、环状 parent、丢失来源、空 Evidence 数组、断裂 current 指针、number 不连续都必须拒绝。JSON Schema 只验证形状，不验证 FK、鉴权、时间先后、CAS、完整来源图；由用例与数据库约束及集成测试共同承担。

事实分类：Event.kind = raw_event / external_document / user_statement / system_action；Evidence.source_kind = raw_event / external_document / user_statement / model_output / fictional_setting。Revision.assertion_kind = extracted_fact / model_inference / fictional_setting / user_assertion。模型输出不能自动标成 observed fact；推断必须有原始输入 Evidence、模型产物 Evidence 和模型/提示版本的可擦除 Provenance Payload。虚构设定只能在显式 fiction 查询域可见。

时间采用双时间：valid_from/valid_to 是半开业务有效区间，recorded_at 为事务接纳时间，knowledge_seq 为已提交知识顺序，occurred_at 是来源事件时刻。过去事实修订追加新版，不重写旧版；as_of 使用服务端返回的知识提交 token，按 knowledge_seq 选择对应快照内的 revision，再按 valid_at 过滤，不把未来修订泄漏到过去。无 valid_to 表示无界，不表示永久正确。

修订保存历史但删除例外：所有版本内容可被法定擦除，遗留结构不可重新构造原文。归档/逻辑删除不等于擦除。

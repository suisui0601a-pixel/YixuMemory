# P0-SC R1/R14 交付与审查边界

Status: Proposal / awaiting Luna independent review，PR 必须保持未合并。
Base: dd34fe5aaf73663779b4f545f6428322039a892d。

## R1

P1 首版只接纳 memory_graph/evidence_graph/source_graph 的同 Space 显式对象关系图，并保留其全部受控副本、历史/派生、key/backup 回执门槛。来源删除反向闭包使全部受影响 Revision 永久失效、Memory source_blocked；memory-only 保留独立来源与其他合法共享引用。替代来源只能显式 CAS 追加新版本，不能恢复旧 Revision。

跨 Space 来源/主体全面擦除明确不支持，source_all_copies/subject_all_copies 或 tenant 范围执行前拒绝，不部分执行、不将 scope 缩为当前 Space 后返回 completed。部分存储失败、缺登记/回执、旧 generation 等保持 blocked 与拒读。canonical source identity/replica registry/跨 Space 协调是未来独立设计。

## R14

客户端 classification 是声明，synthetic 不是安全等级或免加密权。服务端 join Space 下限、来源/Event/Evidence/派生和既有保护；未知 sensitive，保护单调。所有生产内容型 Payload（含 personal/合成内容、locator/provenance/投影）安全 backend seal 后才持久 staging；无 backend/key、不可恢复分配意图或 seal 失败拒绝。NoModel 可用离线安全 backend，无密码豁免。

Tstage 先无内容补偿登记，KeyVault allocate/lookup_intent 可幂等协调丢失响应；T1 绑定/重试重新鉴权分类。AAD seal_context 不可变，当前 policy epoch 分离；不以旧分类重用绕过。rollback/孤儿仅可留不可读密文与受限补偿记录，清理无证明 blocked；DB/WAL/临时区/索引元数据/日志/审计/备份不放可恢复内容明文。公开 API 不能选择测试明文 adapter。没有实现任何生产密码或删除 adapter。

## 兼容变化

- 新 Deletion p0-sc-1 范围、闭包版本和完成覆盖必填字段；新增显式 DeletionIntent 草案端点；旧 Memory/Event 擦除入口分别固定到局部操作。
- Memory.state 新 source_blocked，修复须合法新修订；未来 SDK 需同步枚举处理。
- StagedPayload 新有效分类/epoch/encrypted 服务端字段；PayloadInput.classification 名称暂留，语义明确为声明。
- Event/Evidence/Revision 无正文引用 DTO 继续保留，安全等级在 Payload/ProtectionBinding 与完整图中继承，不添加客户端“安全已验证”字段。
- 合成接入分开来源/locator/Revision Payload，避免共享删除/key 单元。旧无范围或保护记录隔离，不能补 completed/encrypted 标志追认；无已发布 SDK/DDL 的无缝迁移承诺。

## 验证与限制

原 449 个测试 ID 保留；新增 tests/test_security_closure.py 的 76 项设计检查，总计 525。现有合成闭环测试只增强一个函数验证独立 Payload，MCP/A2/A3 测试与实现不改动。

检查覆盖：真实 jsonschema 库的形状/必填/范围反例，有界合成共享图、完整闭包授权、读栅栏、受控存储失败、不可逆旧版本、新修订条件、分类 join、模拟 key/seal/AAD/策略重试与孤儿清理失败。布尔可信观察和 encrypted/receipt_ref **不是安全证明**。测试数量不能证明磁盘未泄漏或密码安全。

README 检查（pytest、mypy、ruff、Schema/合并 OpenAPI、diff、安全/密钥扫描）及 fresh archive/原测试保留核验须完成；提交后以 PR 的实际 CI 和 ls-remote SHA 为同步依据，不能从本文件推定未知 CI 已通过。

未运行：P0_SECURITY_ACCEPTANCE 的 SC-RUN-01..08，包含实际 SQLite 关系/事务并发与删除，真实密钥 backend/认证加密及进程 kill/rollback/孤儿恢复，DB/WAL/TEMP/FTS/日志/代理/备份内容检查，权限竞争、旧快照/worker/key 恢复和离线 NoModel。原 PR #3 的 Windows 目录持久化、真实 MCP SDK、OS 竞态、OAuth/缓存/撤权、审批 broker/密钥隔离等仍为未来门槛；不重判其设计失败。

根 MIT LICENSE、main、原 P0 标签、授权 PR #2 保持不变；独立分支提交，不实现完整生产内核、不部署、不改其他项目。

## 修改文件清单

- `README.md`
- `checks/security_closure.py`
- `contracts/README.md`
- `contracts/deployment/README.md`
- `contracts/deployment/agent-extension.openapi.json`
- `contracts/deployment/examples/synthetic-loop.json`
- `contracts/deployment/tools.json`
- `contracts/jsonschema/deletion-request.schema.json`
- `contracts/jsonschema/memory.schema.json`
- `contracts/jsonschema/payload.schema.json`
- `contracts/openapi/openapi.json`
- `contracts/security/README.md`
- `contracts/security/deletion-intent.schema.json`
- `contracts/security/examples.json`
- `contracts/security/policy.json`
- `docs/AGENT_INTEGRATION.md`
- `docs/ARCHITECTURE.md`
- `docs/COMPATIBILITY.md`
- `docs/CONSISTENCY.md`
- `docs/DELETION_SCOPE.md`
- `docs/DEPLOYMENT_CONTRACT.md`
- `docs/DOMAIN_MODEL.md`
- `docs/MEMORY_LIFECYCLE.md`
- `docs/P0_ACCEPTANCE.md`
- `docs/P0_SECURITY_ACCEPTANCE.md`
- `docs/P0_SECURITY_CLOSURE_REPORT.md`
- `docs/PAYLOAD_PROTECTION.md`
- `docs/RISK_REGISTER.md`
- `docs/SECURITY_MODEL.md`
- `docs/SELF_HOSTED_SECURITY.md`
- `docs/SEQUENCES.md`
- `docs/adr/003-erasure.md`
- `docs/adr/010-deletion-scope-boundary.md`
- `docs/adr/011-payload-protection.md`
- `docs/adr/README.md`
- `tests/test_security_closure.py`
- `tests/test_self_hosting_contracts.py`

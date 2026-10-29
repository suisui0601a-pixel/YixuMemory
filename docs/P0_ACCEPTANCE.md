# P0 自动化验收与未来门槛

运行命令见 README，当前 tests 是契约与 SQLite 特性探针；不把 mock/probe 通过当作生产记忆引擎通过。P0 成功条件是文档基线自洽、下表 P0 检查通过、远端同步和 CI 成功；未来实现门槛单独标记。

| ID | 当前可执行检查 | 成功/失败断言 |
|---|---|---|
| P0-01 | test_schema_and_positive_sample / required / strict_shape | 4 个 Schema 自校验与正例通过；缺字段、伪权限字段、坏 UUID/版本拒绝 |
| P0-02 | revision_counterexamples / second_revision_shape | 缺来源、重复证据、坏类型、非法 parent 形状、坏时间拒绝 |
| P0-03 | openapi_valid_and_resource_shapes_do_not_drift | OpenAPI 3.1 完整验证，资源 schema 与源文件一致 |
| P0-04 | openapi_write_preconditions_and_security | 写需幂等，修改/删需 If-Match，认证声明与 Evidence 写链存在 |
| P0-05 | sqlite_probes | 同租户 FK、回滚内 memory+outbox、单写锁与 CAS 失败有效；不代表完整生产 DDL |
| P0-06 | scanner synthetic negative tests + security_scan | 合成 secret/DB/环境文件反例失败，仓库扫描无发现 |
| P0-07 | mypy / ruff / diff --check / gitleaks | 检查工具严格类型、基础质量、空白与真实扫描通过 |
| P0-08 | gh API / ls-remote / workflow result | PUBLIC、main、保护读回、CI success、远端 SHA=HEAD |

## 后续实现必须执行（本轮尚未通过）

以下计划是可编程的测试脚本要求：实现各 adapter 后以统一 pytest fixture 提供 backend、fault injector、clock、principal；同一 suite 参数化 SQLite 与 PostgreSQL，禁止用 mock 替代最终验收。

| ID | 步骤与故障点 | 必须断言 |
|---|---|---|
| P1-01 | 阻断网络和模型，附合法 event/evidence 写入、读取、修订 | CRUD 可用、无 provider 导入；无来源写 422 |
| P1-02 | 100 次同 key 同体并发；同 key 异体；commit 后响应前 kill | 仅一组 event/revision/outbox；结果同 ID；异体 409；过期语义明确 |
| P1-03 | 两客户端同 If-Match；worker side effect 后 ACK 前 kill；租约过期旧 token | 一次成功一次 412；无重复效应；旧 token 无权 ACK/写入 |
| P1-04 | 共享来源关联两 memory，来源擦除中逐存储注入失败 | 两者不再有效可读，失败 blocked，修复重试 completed 前全部副本无内容 |
| P1-05 | 删除后让旧索引任务写入；恢复删除前备份及 key 备份 | 不复活；恢复屏障阻止上线；key 不可恢复；无法证明则 blocked |
| P1-06 | A/B tenant+personal/role/team/project 权限全矩阵，伪 admin、猜 ID、池复用、撤权竞争 | read/search/history/export/evidence/count/job 全部隔离；无 side-channel 泄露 |
| P1-07 | 事实 A→冲突 B→修订 C，晚到过去事件，按 valid_at/as_of 查询 | 冲突双方保留，过去查询不见未来修订，推断/虚构不升级真实事实 |
| P1-08 | backfill 50% crash、checksum 篡改、旧 app 共存 | 幂等续跑；篡改拒绝；不兼容应用 fail closed |
| P1-09 | 重建 index 过程中撤权/删除/追加；切换前 kill | 权威水位一致、删除优先、原子切换、失败继续旧 generation |
| P1-10 | prompt injection、SQL 元字符、私网 URL、超大输入、缓存残留 | 无新增权限/SSRF；有界拒绝；不在日志写内容 |
| P1-11 | staging 上传成功但 DB rollback；wipe 共享 Payload/key；导出处理中撤权 | orphan 到期清理，闭包/key 不误删他人合法内容；撤权阻断继续输出 |

不设未经测量的性能阈值；先记录数据规模、硬件、并发、p50/p95、错误率、恢复范围再做性能 ADR。测试运行只用合成数据。

## P0-SC R1/R14

新增检查与 P1 故障注入表见 [P0_SECURITY_ACCEPTANCE](P0_SECURITY_ACCEPTANCE.md)。P1-04 的“全部副本”仍覆盖已接受请求的全部受控副本；跨 Space 来源/主体全面擦除明确不支持、执行前拒绝，不能以局部完成代替。P1-11 的 orphan 清理必须是仅密文+受保护补偿记录，清理失败 blocked。当前只运行独立图/分类/形状决策反例，不把测试数量当真实删除或密码验证。

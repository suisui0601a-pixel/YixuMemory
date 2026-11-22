# R5 开源参考

沿用 M3 核心架构挑战的固定源码及许可证核验。重读 SQLite 事务文档与 Temporal 版本冲突测试。下面记录采用的设计原则及本阶段不采用的组件。所有实现为本仓库代码。

| 项目、固定提交、许可证 | 参考及采用 | 本阶段不采用 |
|---|---|---|
| [SQLite](https://github.com/sqlite/sqlite) `cb62289207c58596ee8d63b853b3b8a5db7ed323`，Public Domain 核心 | 使用现有捆绑 SQLite 的写事务序列、延迟外键及只读快照。 [SQLite 事务隔离](https://www.sqlite.org/isolation.html) | 单库原子性无法覆盖外部密钥库、文件或快照域。 |
| [Temporal](https://github.com/temporalio/temporal) `e0233ff6658fcab7b4e7ecc1c16af40baf629b78`，MIT 主仓库 | 从带 `PrevRangeID` 的更新/删除冲突测试借鉴存储端拒绝旧代和按键去重。 [固定测试](https://github.com/temporalio/temporal/blob/e0233ff6658fcab7b4e7ecc1c16af40baf629b78/common/persistence/tests/task_queue.go) | 不接入工作流服务。Outbox 行的幂等不等价于外部副作用恰好一次。 |
| [OpenBao](https://github.com/openbao/openbao) `8f868de6c311cde5404b560a593539d894b1ad19`，MPL-2.0 | 参考 KV 先固定 destroyed 元数据再删除版本，以及 Transit 恢复测试中旧备份的影响。 [KV 删除](https://github.com/openbao/openbao/blob/8f868de6c311cde5404b560a593539d894b1ad19/internal/builtin/logical/kv/path_destroy.go) · [Transit 恢复测试](https://github.com/openbao/openbao/blob/8f868de6c311cde5404b560a593539d894b1ad19/internal/builtin/logical/transit/path_restore_test.go) | 不引入 KMS 服务或从旧包装材料恢复出新权限。 |
| [Mem0](https://github.com/mem0ai/mem0) `753496cf1435d76c3a711f3af51303f7fdcb6420`，Apache-2.0 | 历史存储有独立生命周期，因此验证 Source 失效覆盖所有 Revision 和共享关系。 [历史存储](https://github.com/mem0ai/mem0/blob/753496cf1435d76c3a711f3af51303f7fdcb6420/mem0/memory/storage.py) | 不依赖向量存储与历史库之间的普通应用层删除顺序提供安全证明。 |
| [Graphiti](https://github.com/getzep/graphiti) `72b5c59c06607e09ea855be61b6ff5a3446fc951`，Apache-2.0 | 参考时态边与检索隔离测试。R5 继续让历史视图受当前来源资格约束。 [时态边](https://github.com/getzep/graphiti/blob/72b5c59c06607e09ea855be61b6ff5a3446fc951/graphiti_core/edges.py) · [检索隔离测试](https://github.com/getzep/graphiti/blob/72b5c59c06607e09ea855be61b6ff5a3446fc951/tests/utils/search/test_search_security.py) | 不引入图数据库。M2 的双时态与签名快照沿用现有实现。 |
| [etcd](https://github.com/etcd-io/etcd) `1573d59416efa220421c451f5266b24a5d8374ef`，Apache-2.0 | 学习恢复 revision bump、compaction 标记和失效旧缓存。 [固定恢复测试](https://github.com/etcd-io/etcd/blob/1573d59416efa220421c451f5266b24a5d8374ef/tests/integration/clientv3/watch/v3_watch_restore_test.go) | 不引入集群或在线 Witness。恢复后加大版本无法证明本地撤销账本未回滚。 |
| [restic](https://github.com/restic/restic) `dd09b6bd01af220b1d5d95c0d4d7d472e05e05c4`，BSD-2-Clause | 参考独立密钥包装与备份清单的生命周期，分别登记备份过期、密钥不可用及密文清除。 [包装实现](https://github.com/restic/restic/blob/dd09b6bd01af220b1d5d95c0d4d7d472e05e05c4/internal/repository/key.go) · [存储设计](https://github.com/restic/restic/blob/dd09b6bd01af220b1d5d95c0d4d7d472e05e05c4/doc/design.rst) | 不接入外部备份工具或把 prune 视为物理擦除证据。 |

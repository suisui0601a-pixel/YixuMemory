# ADR-M3-012 R4 可信派发与合成持久结算

2026-11-17。固定基线 `93fb98263102201c629eb1a916a9c7790d6c9e0c`。R4 采用独立的合成 SQLite 数据库，schema 标识 `R4-SYNTHETIC-1`，通过 R3 crate 的私有测试装配运行。未来生产接入需独立 Profile 提案和审查。

Profile 9 的 migration、checksum、表和触发器原样保留。它的 `terminal_evidence` 与 `output_operation` 不适用于 R4 测试结果。新数据库只允许独占创建，打开已有数据库仅用于只读对账。没有自动迁移或导入旧数据库的路径。Profile 9 和 W3 不依赖新模块。W3 的 AuthContext 发行、内容计划及 KeyVault 路径维持原有边界。R4 使用相同的先检查实时权威、短事务绑定意图、事务外执行、持久回执四步，合成权威行独立于实际 Grant 与业务库。未来接入真实授权时应复用 W3/Ports 并按 ADR 009 验收。

可信入口消耗同一 R3 Core 派发出的 `Scheduled`。合成执行只接受 Inspect 目标，建立 ReleaseIntent 前在一个 SQLite IMMEDIATE 事务重新读取精确匹配的 Principal、Realm、Tenant、Space、Grant、expiry、epoch、Revision、source、key、copy 和 deletion generation。意图绑定 IPC 请求摘要、session、connection ordinal、incarnation、operation、attempt 以及本次版本快照。IPC 描述符无法换取 AuthContext 或业务写权限。

本地 SQLite 只提供合成 realm 内的排序。单在途约束和待结算状态保留逻辑配额。真实服务隔离、物理 I/O 结算、独立恢复域与 R12 均需各自验收。完成证据 `SyntheticCompletion` 仅由私有合成执行器构造，回执明确标记合成结果。文件和部署权限仍由 ADR 008 约束。

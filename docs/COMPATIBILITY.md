# 双后端与协议兼容

| 维度 | SQLite 默认 | PostgreSQL 生产规划 | 可移植契约 |
|---|---|---|---|
| 标识/时间 | TEXT UUID/UTC | uuid/timestamptz | 领域 UUID 字符串、UTC，毫秒精度 |
| JSON | TEXT+应用校验 | jsonb+应用校验 | 不依赖 JSON 运算语义作为领域不变量 |
| 写冲突 | BEGIN IMMEDIATE、busy_timeout | CAS+根行锁；必要 SERIALIZABLE | Conflict 与 RetryableFailure 分开 |
| FK | 每连接 foreign_keys=ON | 复合 FK/事务约束 | 所有引用带 tenant/space |
| worker | 单写事务 claim | FOR UPDATE SKIP LOCKED | token fencing 相同 |
| 检索 | 先确定性词法扫描，可选 FTS | 词法/可选向量适配器 | 排名不要求后端完全一致；权限与来源必须一致 |
| 隔离 | 本地 WAL、不用网络共享盘 | 连接池隔离+RLS 防御 | 不启用 dirty reads；服务端同样鉴权 |
| DDL | 部分变更需复制表/维护窗 | 分阶段迁移 | schema version/checksum/恢复门槛一致 |

SQLite 锁及并发行为依据 [SQLite isolation](https://www.sqlite.org/isolation.html)；PG 事务重试与隔离选择依据 [PG transaction isolation](https://www.postgresql.org/docs/current/transaction-iso.html)。这些文档不是本项目实现或性能验证。

REST 使用 OpenAPI 3.1.0 和 JSON Schema 2020-12，以 [OpenAPI 规范](https://spec.openapis.org/oas/v3.1.0.html) 为边界。`/v1` 表示契约主版本，与 `0.0.0-p0` 软件版本分离。ID 不允许 SDK 假设顺序；枚举新增可能破坏 exhaustive switch，主版本内必须先发布 SDK tolerant reader 指引。写入拒绝未知字段；DTO 的可选新读字段须先测试旧客户端；破坏性更改升主版本。

MCP、Python SDK、TS SDK 未来映射同样的 Idempotency-Key、expected version、AuthContext、错误类别与 commit token；MCP 工具参数不授予权限。API 用 page cursor 而非 offset；固定排序为 recorded_at/id 并绑快照。future vector search 不更改权威读取语义。

候选评估：单 DB 最简且可验证，推荐；事件全量重放为唯一真相会与敏感擦除冲突，拒绝用于恢复已擦除内容；按租户独立 DB 隔离更强但运维昂贵，保留 adapter 扩展。队列先用 DB outbox，外部 broker 仅作通知，不成为写入真相。PostgreSQL 后端尚未实现，P0 只运行 SQLite 特性探针，不能宣称双后端已通过。

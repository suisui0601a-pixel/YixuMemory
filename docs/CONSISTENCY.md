# 一致性、事务与恢复

权威对象在单租户单数据库事务内强一致；跨记忆语义不承诺全局线性化。索引、模型和外部擦除最终一致。读己之写通过权威查询和 commit token 保证；检索若索引水位不足则退回权威词法查询或返回 retryable 503，不静默返回假完整结果。

## 写入事务

T1：认证上下文解析 → BEGIN → 锁 tenant/space 的 policy epoch 与删除栅栏 → 鉴权 → reserve 幂等键 → 校验证据/FK → 插入 Event/Evidence/Payload 元数据 → 插入 Revision → CAS Memory.current/version → Outbox/Audit → 幂等结果引用 → COMMIT。首次 Memory/current 循环引用使用事务内延迟 FK 或 staged insert+提交前 invariant 检查，禁止中间提交。外部内容先经安全 KeyVault 加密后写隔离 staging；Tstage 先持久登记无内容补偿意图，Tseal 记录密文元数据，T1 重验当前分类/权限/保护 epoch 并关联 generation。原文不进入持久 staging，未关联对象立即不可读，TTL 调度清理但无回执不视为已清理。模型/外部网络不在 T1。

幂等键作用域 `(tenant,actor,operation,key)`，保留 30 天并在 API 返回有效期；相同 key+相同规范化 digest 返回同一资源引用，相同 key+不同请求 409。超过有效期是新命令，客户端不可假设永久去重。重放结果重新授权，不缓存敏感明文；被删除后返回 410（仅对仍有权限的原操作者），不得复活。请求摘要用服务端 HMAC 并可销毁关联 key，避免低熵内容字典攻击；记录不携带内容。

并发修订：If-Match 对应 version，条件更新受影响行数必须为 1；两写同 version 仅一个成功。事务失败整个回滚，重试重新读权限与版本，语义冲突不能作为技术故障自动覆盖。PG 涉及跨行 predicate 用 SERIALIZABLE 或锁定授权根+聚合根；SQLite BEGIN IMMEDIATE。授权更改也锁相同根，防 TOCTOU。

## 异步和外部副作用

事务 outbox 至少一次投递；租约过期重新 claim，fencing token 与 generation 拒绝旧 worker 提交。外部存储也必须支持条件 generation/upsert 或由单写代理 fencing，只有 DB fencing 不足以阻止旧 worker 复活对象。worker 在操作前后检查删除栅栏；检索最终水合仍过滤权威权限和 tombstone。

默认瞬态重试最多 8 次，base 1s、上限 5min、full jitter；401/403、schema、冲突转 dead/业务错误；429 遵循 Retry-After。租约 60s、每 20s 延长；超时停止操作。宕机后扫描 pending 与 expired leased；ACK 前死机可重复执行，副作用以 `(job_id,generation)` 去重。删除任务 dead/blocked 永不放行读，运维升级处理，不承诺完成时间。

## 删除协调

Tdel 原子标记 deletion_pending、冻结目标及所有派生 Revision/Index/缓存/导出/作业引用，撤销读取能力，写 tombstone/outbox。共享 Evidence/受控来源删除先计算同 Space 全部明确依赖与共享物理 Payload 闭包，完整授权后原子栅栏，所有依赖 Revision 追加永久失效记录、Memory 进入 source_blocked。替代证据不能自动恢复原 Revision，须相关删除 completed 后显式追加有效新版本。Memory 删除擦除独占及显式派生内容，保留独立 Event/Evidence 与其他合法目标共享的原始来源。来源/主体全面擦除的“全部副本和依赖”安全要求不撤销：P1 首版无法枚举跨 Space 独立副本，故 source_all_copies/subject_all_copies 执行前明确拒绝；不得缩为当前 Space 再返回 completed。局部 source_graph 仅指当前 Space 的 Event 根及完整受控副本，必须以新范围回执明确表述。

Worker 擦除 Payload 密文（若检测到遗留明文则按安全事件隔离处理）、密钥（包括 key 备份）、索引、日志映射、缓存、导出、附件、临时文件及 staged 内容；扫描旧 generation，确认无在途 worker 后验证。Event/Evidence 最小信封不含原文、原始 URI、可识别人名或稳定内容 hash。用户映射擦除或不可逆去关联；审计的残余元数据仍有再识别风险，须最小化并受访问控制。

备份采用租户内可独立删除的 payload key；元数据备份包含 tombstone 但不能恢复已销毁 key。可恢复的明文备份/密钥副本未清理时不得 completed；若提供商不能证明密钥撤销和副本清除，保持 blocked，直到备份到期与实际清理回执。SSD/VACUUM 不保证物理介质消磁；仅承诺经测试的应用可恢复范围，介质由运维销毁策略覆盖。P1 全部生产内容型 Payload 都先加密，包括客户端 synthetic 声明；DB/WAL/临时区、索引元数据、日志、审计和备份不放可恢复内容明文。无安全 KeyVault、key 不可用或 seal 失败拒绝内容写入，不降级存明文；NoModel 也须安全离线或在线 backend。客户端 classification 不可信，绑定/重试重新 join 当前服务端保护；rollback 后密文孤儿不可读且保留补偿登记至清理回执验证。

## 迁移、重建、恢复

Migration version+checksum、单执行者锁、expand→backfill（可恢复游标）→verify→contract；旧/新应用兼容窗口明示，不支持自动破坏式 down migration。失败先停写，恢复已验证备份或前向修复；迁移前验证恢复演练。SQLite 需要复制表时用独占维护窗口，PG 并发建索引按 adapter 独立阶段；不虚称 DDL 事务完全等价。

重建从权威 revision+删除水位快照生成新 IndexGeneration，追 outbox 至目标水位，验证租户/授权 epoch 后原子切换；删除 tombstone 必须优先重放。恢复进入隔离环境，校验 schema/checksum、应用外部受保护删除登记与 revoked keys、清理禁用内容再开放访问；缺失最新删除登记就拒绝恢复上线。RPO/RTO 暂无实测值。

## 故障注入断言

COMMIT 前 kill→无半个版本；COMMIT 后响应前 kill→重放同结果；index 成功 ACK 前 kill→无重复；旧 worker 在 delete 后写→被 generation 拒绝或无法被读取且须清理；权限撤销与读取竞争→只按已定义锁序串行结果；恢复老备份→无法获取删除 key。见验收表逐项实现。

## 知识顺序与墙钟

Tenant.knowledge_seq 是事务内更新的计数器，所有权威写锁定该 tenant 根至 COMMIT，分配 Event/Revision 的 knowledge_seq；回滚不消耗已提交值。不能用数据库 sequence 的分配顺序冒充提交顺序。此选择牺牲单租户并发度以获得 P0 可验证顺序，性能未测量。多租户没有全局排序。

recorded_at 在根锁内由服务端 Clock 分配，表示事务接纳时刻，**不保证等于物理提交时刻**。as_of 是签名的 tenant/knowledge_seq token，不接受任意墙钟来伪造精确历史可见性；时间展示与业务有效区间仍用 UTC。无 as_of 时读取当前一致快照。后续若增加墙钟历史查询，必须定义时间到提交序的映射及不确定窗口并附 ADR。历史删除/撤权永远应用当前策略，不因 as_of 绕过。

P0-SC 操作类别、闭包快照/完成覆盖、共享来源归属和跨 Space 拒绝见 [DELETION_SCOPE](DELETION_SCOPE.md)。生产分类与 KeyVault/staging 故障边界见 [PAYLOAD_PROTECTION](PAYLOAD_PROTECTION.md)。这是明确限定可接受操作，并未把任何受控副本排除在已接受请求的“全部副本”门槛外；缺登记/证据时拒绝或 blocked。

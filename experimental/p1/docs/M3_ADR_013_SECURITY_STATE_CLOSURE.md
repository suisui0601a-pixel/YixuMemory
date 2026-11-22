# ADR-M3-013 本地安全事件账本与删除闭包

2026-11-18，合成实验。基线 `0b489e0de53cbf2ef3786e88b58d318778e7cd92`。

R5 复用 R4 的一个 realm 排序器、活跃释放槽、`ReleaseIntent`、合成终态及不可变结算回执。R5 从空文件独占创建附加 SQL 表，以 SQLite `application_id=1498952245` 区别 R4 数据库。R4 的基础 SQL、Profile 9、迁移 checksum 和正常编译图保持原样。R5 的诊断入口只打开自己的格式，使用一个只读事务获得一致快照。历史文件、备份和旧 Profile 都没有升级入口。

受信宿主通过私有类型提交安全命令。命令以固定编码绑定请求 ID、作用域、类型和必要的世代或期望 epoch。同 ID 同命令返回原事件，同 ID 异命令返回 Conflict。顺序以 SQLite 提交的事件 ID 为准。Closing 和请求取消先持久提交，旧会话随后失效。只有 R4 结算回执确认活跃输出已结算且较早事件都 Applied 时才能进入 EffectiveFence。最后一个写事务提交授权投影、业务安全状态、墓碑、Outbox 和 Applied 回执。SQLite 延迟外键要求事务内的变更上下文及回执同时落地。过期 Grant 或失效的 Current Revision CAS 会让 owner 保持阻断。

删除计算区分拥有关系和证据依赖。删除 Memory 只保留其拥有关系下的闭包，不沿 Revision 引用反向清理共享证据。删除 Source 或 Evidence 会使引用该证据的所有 Revision 和其拥有的 Payload 失效。墓碑以对象及世代为键，不能由旧历史记录覆盖。新 Current Revision 必须属同一 Memory、前进至更高世代且具有完整的有效依赖。

R5 在 R4 的派发事务内重查 Current Revision 和精确读取依赖，并把排序后的对象 ID 和世代摘要纳入 ReleaseIntent。缺少这段绑定时，R5 数据库拒绝按 R4 旧入口派发。

逻辑失效、合成 KeyUnavailable、合成 CiphertextRemoved 和合成 BackupExpired 使用独立证据。重叠事件共享同一对象世代的 Outbox 工作及回执，不能重复计数；副本清理必须等同一 Payload 的密钥不可用回执。观察和回执分别持久化。备份到期需要单独观察，不能由本机时钟或条目不存在推断。

考虑了自动接管、直接扩展 Profile 9，以及在业务表和独立安全账本之间异步写入。当前缺少可信所有者重建与防回滚证据，自动接管可能复活旧请求。两个可独立提交的账本会留下可见性与安全变更之间的窗口。独立合成格式允许先检验单事务和重启对账，代价是未能覆盖真实领域表与物理副作用之间的对账。Local 保持离线，当前阶段已有文件的诊断权限不包含新的运行权。生产扩展需要新迁移提案和独立验收。

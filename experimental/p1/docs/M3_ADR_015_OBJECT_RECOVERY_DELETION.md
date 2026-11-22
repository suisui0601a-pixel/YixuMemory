# ADR-M3-015 B3 对象隔离与删除恢复

2026-11-19。基线 31549a5b6393c31f2d5bac40b3a9f27619eddb24。Windows NTFS 私有实验。

B3 新建 Profile 11，复用完整 Profile 10 DDL 后安装受控扩展。Profile 9/10 定义和已有文件保持不变。没有自动升级旧数据路径。现有 Profile 10 的读取/修订和严格恢复测试继续运行。Profile 11 打开时使用编译 DDL 核验全部实际 schema、checksum、FK 与完整性。此阶段不提供生产迁移工具。

对象损坏与共享安全故障分开。隔离记录持久化，绑定 operation、Memory、Revision、Payload、Key、世代、原因和观察 owner。它不改变旧业务行、manifest、Current 指针或正文。有效隔离持续拒绝对应 Revision；使用其他有效证据的新 Revision 必须显式建立。未知 schema、FK、epoch、Receipt/Outbox、安全账本、owner 或未完成写入仍全局阻断。

Profile 11 对 FK 故障仅允许两类进入对象诊断：revision 指向缺失 revision_evidence，或 revision_evidence 指向缺失 evidence。这些记录完整保留，由实际 operation 关联隔离；其余 FK 破坏继续全局拒绝。新写入仍强制 foreign_keys=ON。Profile 10 对全部 FK 故障保持原严格拒绝。

恢复先验证原停机账本与可信 supervisor 的实际退出证明。文件核验和对象诊断在写事务外完成；在消费 stop_receipt 的同一事务中重查原账本、持久化新隔离、写 resume_receipt 和新 incarnation。隔离纳入下一次停机摘要。非正常终止的恢复只有诊断权限。不能通过变小 manifest 或忽略历史丢失获得可用性。

删除入口使用内部 Application 的可信身份和当前 Grant。规范请求绑定作用域、主体、目标类型、ID 和请求键。跨 Space 全面擦除明确拒绝。Closing 先提交，停止 realm 新读取与写入；EffectiveFence 后在一个事务中计算并登记同 Space 完整闭包、墓碑、逐对象 copy/key/backup 工作、Applied、epoch 与不可变回执。并发删除按事件 ID 排序，重复请求返回原状态。删除不改变业务 Knowledge Sequence、Revision 历史和 Current 指针。

Memory 闭包覆盖其全部 Revision/Payload/Key/Copy/备份登记，保留独立 Source/Evidence。Source 闭包覆盖明确证据及全部依赖历史 Revision/Payload，并永久阻断依赖，新的 Revision 仍受当前 Source/证据规则检查。

Key marker 字节采用固定版本格式，绑定安装、scope、Revision、Payload、Key、Generation、删除意图和第一次结算 owner。先持久化不可变工作身份，再发布 marker，验证字节与对象身份后写观察和回执。允许重叠请求引用相同工作；一个请求不能清除另一个栅栏。未知 marker、错配字节或无法验证的消失结果保持阻断。既有 W2 不同格式 marker 没有自动转换。

逻辑失效、KeyUnavailable、CiphertextRemoved、WrapperRemoved、BackupExpired 分别记账。W1 删除使用已验证句柄和预期密文字节。文件操作在 SQLite 写事务外执行；恢复需要与持久意图和观察对账。合法 marker 永久保留。密钥包装清除和密文清除仍不等于物理介质擦除、独立备份终结或密码学不可恢复。

Local 全离线，所有本机文件属于同一回滚域，不能证明完整一致旧快照最新性。缺少当前可信 supervisor 退出证明时恢复拒绝。独立 Witness、生产服务身份、外部输出、强删除和 R12 均不在本阶段。

Outbox 修订语义由 Profile 11 的受控视图输出 MemoryCreated/RevisionCreated 版本化事件。旧 Profile10 action 不重写。消费者必须通过该视图及 Receipt/Revision 关系处理，不能仅检查旧字符串。

受控本地备份为 W1 发布的加密密文及 DPAPI 包装组合文件。登记保存 operation、Backup ID、Generation、文件摘要及句柄身份。副本清除后仍单独保留备份待办，只有同一登记备份经 W1 身份验证删除并确认缺失，才记录 BackupExpired。外部导出、离线未登记备份和物理介质擦除不包含在该回执。

未结算的登记备份意外缺失，不能据此签发清除回执；保持 pending 并阻断恢复。实际删除后、回执提交前崩溃也不推断成功。原密文或包装已存在清除回执却再次出现时，与备份再现一样全局阻断。无回执的原密文/包装缺失不能被 cleanup 自动当成已执行删除。

验证包括独立过期读取拒绝、多对象隔离、共享安全故障、重叠删除、来源闭包、marker 篡改、逐阶段进程终止、恢复二次终止、真实 NTFS 文件/DPAPI、SQLite 原子回滚、默认/全特性与公开 Python 基线。合成备份登记如未有真实可验证清除回执就必须报告待处理。

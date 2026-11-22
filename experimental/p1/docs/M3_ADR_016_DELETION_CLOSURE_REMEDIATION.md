# ADR-M3-016 F1/F2 定点修复

2026-11-22，失败基线 5fdfe3ef4c9515589531946deb876c78e33494d0。仅 Windows NTFS 合成实验；待独立审查。

F1：使用唯一 authoritative_closure，从不可变 committed write_intent 取得候选，验证 Source 的 Revision/Payload/RevisionEvidence 关系。不能可信确定完整集合时，在任何 Closing 写入前拒绝。隔离不授予删除权限，Key 损坏但关系完整的对象仍必须逐 Memory 检查当前 Grant。

Closing 事务将 operation 集合存入不可变 deletion_closure，纳入停机账本摘要。Fence、Applied、Reconcile 使用同一计算器并比较冻结集合；Applied 同一事务再验证当前 Grant，并检查执行 member 与冻结集合一致。不能靠缩小集合恢复可用性。

P11 受控 Schema 修订：新增 deletion_closure 及 Closing/scope 约束和生成的 immutable triggers。旧 P11 文件不自动升级或重新解释；完整 schema attestation/checksum 会拒绝旧定义。此阶段仅使用新建空实验目录，无生产迁移器。P9/P10 DDL、既有数据、W1/W2/W3 和 Cargo.lock 不变。该兼容性限制必须交独立审查。

F2：Marker 权威判断须区分未记录身份、已记录未结算、完整回执。身份或观察已持久化后，Marker 缺失即共享故障；读写准入、Reconcile 和 cleanup 使用同一判定。不重新发布已丢失身份的 Marker 来修复。完整匹配合法回执不因此失效，未知状态不自动结算。

验证：先执行旧版失败反例，随后补关系完整且授权覆盖全部集合的成功场景、依赖不确定拒绝、Grant 单独失效拒绝，以及活跃 Owner 的 KeyObserved→Marker 缺失场景。保留原日志；Native 输出、A3/R12、生产恢复、Witness 和强删除保证均未获放行。

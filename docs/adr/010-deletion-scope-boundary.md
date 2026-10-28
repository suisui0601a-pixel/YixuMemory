# ADR-010 删除图与跨 Space 来源边界

Status: Proposed — awaiting independent review (design only)
Date: 2026-10-28 · Base: dd34fe5aaf73663779b4f545f6428322039a892d

R1：原 Deletion 的目标+status 无法说明完成覆盖。选择同 Space 显式对象/依赖图、所有受控副本回执、不可逆 RevisionInvalidation；Memory 删除保留独立共享来源，source/Evidence 删除反向闭包失效。新增 source_blocked 与受检新修订恢复路径。广义来源/主体擦除明确拒绝，不缩窄请求。详见 [范围设计](../DELETION_SCOPE.md)。

候选：全局语义匹配（无法证明完整、误删风险）/把所有删除理解为当前 Space（虚假完成）/显式操作与范围且广义拒绝（选择，能力较少但可验证）。未来 canonical identity/replica registry/跨 Space 授权与栅栏另立 ADR。

补充并收紧 ADR-003，不撤销“全部受控副本完成才可 completed”。兼容变化：Deletion 新必填 p0-sc-1 字段、Memory 新状态、显式 intent API；旧无范围记录不能默认迁移为 completed，客户端需更新。

可执行失败验证：共享 Evidence 两 Memory 均失效；Memory A 删除保留 B 来源；独立 Space 复制不能 tenant completed；缺回执 blocked；未获完整闭包权限无栅栏/擦除；替代证据只建立新 Revision。纯图 oracle 不是擦除实现；实际存储/备份/key/恢复与并发验收见 P0_SECURITY_ACCEPTANCE。

# ADR-010 删除图与跨 Space 来源边界

Status: Proposed — awaiting independent review (design only)
Date: 2026-10-28 · Base: dd34fe5aaf73663779b4f545f6428322039a892d

R1：原 Deletion 的目标+status 无法说明完成覆盖。选择同 Space 显式对象/依赖图、所有受控副本回执、不可逆 RevisionInvalidation；Memory 删除保留独立共享来源，source/Evidence 删除反向闭包失效。新增 source_blocked 与受检新修订恢复路径。广义来源/主体擦除明确拒绝，不缩窄请求。详见 [范围设计](../DELETION_SCOPE.md)。

候选：全局语义匹配（无法证明完整、误删风险）/把所有删除理解为当前 Space（虚假完成）/显式操作与范围且广义拒绝（选择，能力较少但可验证）。未来 canonical identity/replica registry/跨 Space 授权与栅栏另立 ADR。

补充并收紧 ADR-003，不撤销“全部受控副本完成才可 completed”。兼容变化：Deletion 新必填 p0-sc-1 字段、Memory 新状态、显式 intent API；旧无范围记录不能默认迁移为 completed，客户端需更新。

可执行失败验证：共享 Evidence 两 Memory 均失效；Memory A 删除保留 B 来源；独立 Space 复制不能 tenant completed；缺回执 blocked；未获完整闭包权限无栅栏/擦除；替代证据只建立新 Revision。纯图 oracle 不是擦除实现；实际存储/备份/key/恢复与并发验收见 P0_SECURITY_ACCEPTANCE。

## 2026-10-29 R1 类型校验定点修复（待 Luna 复审）

前序 4ecba8d 的图 oracle 只检查存在性，Evidence.event_id 指向 Memory 仍可能 admit_graph。选择整图端点矩阵验证后才遍历，未知字段/类型/悬空引用/必需行缺失均无部分 fence/erase，并保持原图。拒绝仅修复单个 ID 反例、跳过异常边或只检查本次路径。矩阵和当前 Revision 派生投影的依据见 DELETION_SCOPE；合法共享来源、历史 Revision、三类内容实体间派生和 Memory-only 既有行为保留；显式派生 Event 的捕获边始终传播，不能因删除根是 Memory 就漏掉 Evidence。没有 R14 架构/接口修改，也没有真实 DB/删除实现。验证需先看到旧代码新反例失败，再执行原 525 项及新增类型矩阵、路径外错误和合法闭包回归。

# ADR-011 服务端分类与全部生产 Payload 加密

Status: Proposed — awaiting independent review (design only)
Date: 2026-10-28 · Base: dd34fe5aaf73663779b4f545f6428322039a892d

R14：客户端 synthetic 声明不能批准明文。选择服务端单调 join 分类，未知 sensitive，所有生产内容（含 locator/provenance/派生）安全 backend 加密后持久化；无 backend/key、seal 失败 fail closed。NoModel 可配离线安全 backend，不豁免。详见 [Payload 设计](../PAYLOAD_PROTECTION.md)。

候选：只加密 sensitive（误分类可绕过）/客户端控制 synthetic 例外（不可接受）/全内容加密+服务端分类（选择，key backend 成为生产内容路径前提）；全盘/数据库加密仅可附加，不能独立满足单对象删除。

沿用 KeyVault/PayloadStore/UoW 和事务边界；Tstage 预登记补偿、仅密文 staging、T1 重新校验保护与幂等上下文。DB/WAL/索引/日志/审计/备份不放可恢复明文。默认不以原生明文 FTS 实现生产检索，有界内存词法路径保留 NoModel；性能未测。安全 profile/具体 backend 需 P1 验收，不自造密码实现。

兼容变化：staging response 新服务端保护字段；旧保护记录缺失即隔离，不能补标志追认；classification 字段暂留为声明。Event/Evidence/Revision 引用 DTO 不变，当前有效分类通过受信 ProtectionBinding/来源图计算，不原地改不可变正文。

可执行失败验证：synthetic 仍需加密、低声明引用敏感 Evidence 不降级、缺/失效 key 拒绝、seal 失败/rollback 清理未证实则 blocked、旧 epoch/旧分类重试拒绝。当前只测试决策模型与形状；真实密文/文件字节/故障恢复/密钥隔离未运行。

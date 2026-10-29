# P0-SC 检查与 P1 运行验收

Status: design checks only / pending independent review。仅收敛 R1/R14，不重开已接受部署/MCP/事务/授权设计。没有运行记忆内核、加密 adapter 或真实擦除协调器。旧 449 项回归继续执行，不用新增数量证明安全。

| 当前独立契约检查 | 反例/断言 | 能证明的范围 |
|---|---|---|
| deletion shape / scope | 缺 scope、completed+pending coverage、错误目标配对、伪 tenant completion 拒绝 | DTO 形状，不证明闭包或回执真实 |
| explicit graph | 同 Space 共享来源的两 Revision 都进入 fence；Memory A 保留 B 的 source；显式派生闭包 | 有界合成图，不证明数据库完整登记/并发 |
| broad/denied intent | tenant/source_all_copies/subject_all_copies 拒绝，缺授权不产生任何 fence | 纯决策函数，不是真实鉴权 |
| completion/read gates | 每个登记存储失败/缺失、旧 generation/活 worker → blocked；所有读面持续拒绝；替代来源不恢复旧 Revision | 模拟可信观察，不是真实存储/缓存 |
| classification | 下限/来源/Evidence/派生 join；unknown/synthetic 不降级；既有敏感保护单调 | 标签决策，非内容检测/加密证明 |
| production write / cleanup | 缺 key/backend、seal/AAD 失败、旧 epoch、plaintext adapter、rollback 清理缺证明拒绝/blocked | 模拟观察，无密码/落盘/授权执行 |
| API drift | Schema 自校验、Deletion 嵌入一致、staging 新字段与合成 fixture 一致；无公开免加密字段 | 机器契约一致，非运行保证 |

README 的 pytest、mypy、ruff、安全扫描、diff/gitleaks/CI 全部执行。观察中的 snapshot_complete/secure_backend/seal_ok 等只由测试模拟未来受信结果，不得把传入 true 当真实权限、完整图或加密证明。

## 必须留给 P1 的实际验收（本轮均未执行）

| ID | 实际步骤/故障注入 | 必须结果 |
|---|---|---|
| SC-RUN-01 | SQLite 实际 DDL+UoW 建两个共享 Evidence 的 Memory、历史/派生/共享 Payload；各读面并发读与擦除交错 | 原子完整 fence，无漏引用；旧 as_of/cache/索引无法水合；Memory-only 不误删来源 |
| SC-RUN-02 | 每种实际存储、export、backup、key 副本、worker generation 故障及恢复旧备份；删除中并发新增 link | blocked 不放行；缺证明不 completed；恢复/晚 worker 不复活；未知登记不缩范围 |
| SC-RUN-03 | 两 Space 相同原文独立导入，来源/主体 tenant 请求；权限在预检/Tdel 交错 | 广义执行前拒绝，无部分副作用；范围不能降级；无权限对象信息不泄漏 |
| SC-RUN-04 | 实际安全 KeyVault/PayloadStore 断开/过期/key 丢失/nonce重复/AAD篡改/seal 失败 | 不落明文、不提交部分对象；认证密文不可跨 tenant/Space/generation 解密；无旁路 |
| SC-RUN-05 | 合成 canary 内容贯穿 staging→attach→revision→index/search→export；检查 DB/WAL/journal/TEMP/日志/审计/队列/备份、代理缓冲/转储配置 | 无可恢复内容明文/词项/定位；加密不等于可删除性，全部 key 副本独立验证 |
| SC-RUN-06 | Tstage/allocate/seal/publish/Tseal/T1 每个断点 kill、磁盘满、rollback、cleanup fail、旧 epoch 幂等重试 | 仅密文孤儿+受保护补偿意图，立即不可读；清理无回执 blocked，非盲目 TTL 成功；无错误 key/对象擦除 |
| SC-RUN-07 | source_blocked 后加入独立证据，尝试改旧版/旧摘要复活，再合法 CAS n+1 | 旧 Revision 永久失效；所有相关删除完成且新来源有效才恢复新 current |
| SC-RUN-08 | 离线/模型网络阻断、生产 synthetic 声明、测试装配与生产装配混用 | NoModel 仍可加密读写/有界确定性检索；无安全 backend 拒绝；API 不能启用明文测试适配器 |

这些运行门槛补充 P0_ACCEPTANCE P1-04/05/11，不取代此前 MCP/OS/审批 broker 的未关闭运行事项，也不重新判定其设计失败。SQLite 契约探针不是本表生产实现验收；最终报告须分别列出契约通过与运行未执行。

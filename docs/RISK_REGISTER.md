# 独立反例审查清单

请审查者构造可执行交错、最小输入和失败断言，不以“文档看起来完整”代替验证。

| ID | 风险/反例 | 预期与剩余问题 |
|---|---|---|
| R1 | 共享 Evidence 残留有效依赖、Memory-only 误擦共享来源、跨 Space 局部完成冒充全面擦除 | ADR-010 Proposal：同 Space 显式图+全部受控副本；广义执行前拒绝，部分失败 blocked；永久旧版失效/新修订恢复；纯图契约检查不证明实际关系完整与擦除 |
| R2 | 旧 worker 在擦除后向外部索引写回 | generation 拒绝或水合不可读且清理；外部存储 fencing 能力需评审 |
| R3 | 老备份恢复 key 或删除登记不完整 | 隔离恢复拒绝上线；离线快照、供应商副本证明待演练 |
| R4 | HMAC/时间/opaque ID 仍可再识别 | 最小信封不等同匿名；保留期与去关联需隐私评审 |
| R5 | auth epoch 在检索候选与水合间变化 | 重验/中止，count 和 cursor 不能泄露；实现待竞争测试 |
| R6 | SQLite 锁行为被当作 PG 等价 | 双后端共同不变量套件未实现；禁止据此做性能宣传 |
| R7 | JSON Schema 通过但 FK/时间/分类关系错误 | 跨对象与因果校验在领域层；当前测试不能证明这些已实现 |
| R8 | 幂等 30 天后重试或删除后重放 | 明示去重期限，授权重验，拒绝复活；客户端丢期限的恢复策略待验证 |
| R9 | 模型或文档把虚构提升为事实 | 显式来源、fiction 查询域、人工规则；分类映射需人工审查 |
| R10 | initial push 在保护启用前 | 明示一次 bootstrap；保护读回、CI 成功后正常 PR；管理员仍能修改保护 |
| R11 | Git 内容/依赖被注入 secret | 合成扫描+gitleaks+diff 审查仍非绝对证明；依赖固定与后续更新需审计 |
| R12 | 全局时序、as_of 与晚到事件互相混淆 | recorded_at 为事务接纳时间，as_of 为 tenant knowledge_seq token；根锁与提交序实现待验证 |
| R13 | orphan staged blob 或跨空间证据漏清理 | staging TTL、同空间 FK、擦除闭包扫描；上传取消故障需验收 |
| R14 | synthetic 伪声明、低分类派生、无 key/加密失败/旧分类重试导致明文或降级 | ADR-011 Proposal：服务端 join、未知 sensitive、全部生产 Payload 安全 backend 加密，失败拒绝；真实加密/WAL/临时区/日志/备份/孤儿清理仍待 P1 验收 |

首轮可同步基线不代表上述生产门槛已通过。独立审查后通过 ADR 和最小补丁收敛，不直接扩展引擎功能。

P0-SC 仅收敛 R1/R14，候选边界与失败方法见 DELETION_SCOPE、PAYLOAD_PROTECTION、P0_SECURITY_ACCEPTANCE；尚待 Luna 独立审查，不自称审查通过。跨 Space canonical identity/replica registry 未设计实施，不能把同文本/URL 当完整副本清单。

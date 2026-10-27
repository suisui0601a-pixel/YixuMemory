# ADR-008 自托管与 AI 原生接入

Status: Accepted — design only; runtime acceptance pending
Date: 2026-10-28
Extends: P0-Baseline-001; no supersession of ADR-001..006

## 决策与备选

| 决策 | 备选及取舍 | 选择/兼容影响 | 可执行验证或失败测试 |
|---|---|---|---|
| 部署方式 | 云账号优先方便运维，但依赖官方服务；独立自托管增加平台测试成本 | desktop/container/server+postgres backend preset；个人不限设备、无产品账号或激活 | 四 Profile schema/defaults；未来断网、无账号、多实例冷启动 |
| AI 身份 | 单万能管理员省事但扩大越权；控制/数据面分离更复杂 | 分 audience、credential、OS/helper 与审批边界；不改变原 tenant/Space 授权 | 错 audience/config 拒绝、工具目录无安装管理；未来真实跨权限攻击 |
| 自动化执行 | 任意 shell/--yes 方便但不可审计；类型化 plan 可约束 | doctor→plan→受信 approval→apply，非交互仍验签/范围/漂移，无覆盖未知文件 | schema + plan/approval 语义 oracle，未知路径/过期/主机漂移反例 |
| 默认网络 | 默认公网直连容易接入但风险高 | 原生 loopback+认证；容器私网绑定与 host publish 分开；远端 TLS 代理、DB 不公网 | 非 loopback/native/public DB 反例，未来真实 socket/防火墙探针 |
| 接入协议 | 只 REST 简单但 Agent 需自适配；直接 MCP 访问 DB 绕过用例 | REST+MCP stdio/Streamable HTTP 薄适配器+SDK；同 L2，modern/legacy wire 分离（ADR-009 收紧） | catalog→OpenAPI operation 映射、工具输入 shape、未来真实双时代 metadata/handshake/auth |
| 写链缺口 | 省掉来源或把内容内联入 Memory 更快但违背基线 | 新 staging、capabilities、context 三端点及搜索 watermark header；既有 T1/证据/幂等不变 | 合成 payload→event/evidence→memory→search 引用链及空来源反例 |
| 恢复 | 直接拷 DB/覆盖恢复简单但丢 WAL/删除水位 | 一致备份+隔离恢复+最新删除登记+schema 门槛，journal 保留失败 | schema/readiness与恢复计划反例；未来注入断电/旧快照/撤销 key |
| 验收边界 | 用 fixture 宣称已运行 E2E 易误导 | 当前只验证契约与合成引用，不实现服务/CLI，不宣称实机支持 | 自动化检查无副作用；文档分开列运行验收 |

## 与原 P0 的一致性及开放点

未改变 Event/Revision 不可变与可擦除 Payload、同租户同空间证据、UoW/outbox、CAS、授权根、知识提交序和删除闭包。安装 journal 是控制面外部副作用记录，不能替换业务 UoW。敏感数据 key backend 不可用仍拒绝敏感写；无模型不是无加密或无认证。

本 ADR 补齐 staging/capabilities/context 和可选搜索 watermark header，保留原域资源 DTO；context 增量投影及缓存撤权语义须在实现前接受审查，不能用 fixture 冒充真实上下文读取。自托管 HTTP MCP OAuth issuer、平台 keychain/helper、签名包、实际兼容版本和备份调度均待实现决策；不自行弱化安全去完成验收。

## 关联交付与许可

见 DEPLOYMENT_CONTRACT、AGENT_INTEGRATION、SELF_HOSTED_SECURITY、SELF_HOSTED_ACCEPTANCE，及 contracts/deployment。当前 MIT 不变，既有 tag 不动，PR #2 不合并；其 Proposal 不从未合并分支引入本提交。本轮无生产部署、无完整引擎实现、无其他业务项目改动。

## PR #3 修复补充

原固定单时代、泛化路径重查/配置 CAS 及可扩展 approval 的描述由 [ADR-009](009-pr3-remediation.md) 收紧；双时代 wire、可信句柄文件原语及严格单计划审批见其引用文档。独立重审待完成，原 P0 不变量不变。

# ADR-009 PR #3 协议、文件操作与审批修复

Status: Proposed remediation — awaiting Luna re-review; design/runtime acceptance separate
Date: 2026-10-28
Reviewed predecessor: 3742dbdb51ca6ba0b120a46ca5de48d8d0f1c4da (FAIL)

## 决策、替代与兼容性

- A1：拒绝“只换协议日期”的方案。首发目标 modern 2026-07-28，兼容目标 legacy 2025-11-25；领域工具共用，wire/session handler 分离。modern per-request metadata/discover/HTTP mirrors 与 legacy handshake/session 均建立独立 fixture。代价是双时代维护及真实 SDK 套件；未运行前两者都不宣称已支持。见 MCP_COMPATIBILITY。
- A2：拒绝 realpath+lock 作为恶意路径竞态防线。采用可信锚点、逐层 no-follow、live identity/ACL/volume、handle-relative、exclusive/no-replace 的受限 helper；能力不足 fail closed。通用已有配置 CAS/覆盖不再承诺，connect 初版只发布新管理文件；代价是部分路径/平台不能自动安装。见 FILESYSTEM_OPERATIONS。
- A3：拒绝 approval 隐式超集。单计划 action IDs/scopes 精确集合匹配且无重复，allow_elevation 与计划实际提权相等，并需额外受信安全策略。广义预授权未设计也不接受；未来如需必须独立 ADR/对象，不能把它塞入 single-plan approval。oracle 仍是模拟观察，不是签名/真实授权。

本修复收紧 ADR-008 的协议/路径/配置编辑/审批描述，不改变 P0 UoW、证据、版本、租户、删除与恢复规则。client-config/capabilities 新字段、approval_kind 与 plan anchor 绑定是未发布 P0 草案的显式修订，旧 fixture 缺字段拒绝并重新 plan；无已发布 SDK 兼容承诺。LICENSE、基线 tag 和 PR #2 均不变，PR #3 不合并。

## 验证门槛

契约：保留原 137 项 ID，新增每时代合法/非法请求、错误版本/降级、HTTP mirrors、审批额外动作/scope/提权/重复、目录身份/组件/发布/清理反例；校验机器 schema、API fragment 和文档一致。运行：真实 SDK/OS native helper/竞态/认证/恢复全部仍待执行。提交者只能报告 remediation ready，不能宣布独立审查通过。

## 第三轮 A1 修订（Proposal，待 Luna 第三轮独立复审）

修订前 HEAD：15b12a0f8458d042dc3ef57ac91eb12c14d87624。A2/A3 决策及其 oracle 保持不变。

1. 选择固定官方 2026-07-28/2025-11-25 JSON Schema 的原样定义闭包，离线校验输出，再叠加产品限制，拒绝仅手写宽松断言。DiscoverResult/ListToolsResult 的必填缓存字段不可遗漏；首轮 ttlMs=0/cacheScope=private，HTTP/proxy/SDK 不复用授权相关结果。官方允许 public/正 TTL，不代表本产品要使用它。源 commit、SHA-256 与上游许可一并保留；根 LICENSE 不变。代价是需人工审核上游更新，不能静默浮动依赖。
2. 选择结构化解析与保守 fail-closed，拒绝 response_kind 字符串、普通错误、超时、空 body 作为 legacy 证明。官方传输指南的 fallback 建议更宽泛，不是仅限 -32601/-32602；本产品主动收紧为未知时代失败并提示显式 legacy-only，旧缓存也不足以证明当前时代。现代缓存连接不自动降级。代价是未知旧服务不能全自动识别，需要显式配置；优势是不会把普通故障当旧协议并自动重放。仅初次 server/discover 探测，禁止业务调用试探。

验证：官方必填字段删除必须失败、修复后通过；字段类型/枚举/负 TTL/跨时代、其他已声明结果审计；结构化 error/status/媒体类型/ID/探测阶段/缓存时代矩阵及已报告 -32603 反例。保留原 245 个测试 ID，更新其中错误的 A1 断言，新增测试独立计数。实际 SDK、SSE、认证、缓存代理与 Windows 目录持久化等门槛仍未执行，不能据此宣布审查或运行验收通过。

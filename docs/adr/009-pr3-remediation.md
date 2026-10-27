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

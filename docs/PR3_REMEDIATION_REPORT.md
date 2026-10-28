# PR #3 A1/A2/A3 修复交付记录

Status: remediation prepared; independent re-review pending. 本文不代表 Luna 已通过重审，也不代表真实 MCP/OS 运行验收通过。

审查基线：3742dbdb51ca6ba0b120a46ca5de48d8d0f1c4da。复用现有 docs/self-hosting-agent-native 分支与 PR #3，不合并 PR，不混入 PR #2。

## 修复

- A1：MCP_COMPATIBILITY 明确两时代、逐传输检测、版本错误、HTTP mirrors、授权、工具发现及独立会话。现代服务必须 discover，但客户端不必每次 discover。client-config/capabilities/catalog/API 片段和 wire fixtures 一致；现代为首发目标，旧版为兼容目标，均未实际实现。
- A2：FILESYSTEM_OPERATIONS 定义可信锚点、live handle-relative/no-reparse、owner/ACL/identity/volume、exclusive/no-replace、清理与恢复。未知 FS/原语或可被威胁主体修改祖先时 fail closed；不再承诺通用配置 CAS 覆盖。系统管理员或完整控制同一安全主体者不在此保护假设内。
- A3：精确且无重复的 action/scope 集合，提权相等并受额外安全策略；禁止隐式广义预授权。oracle 仍只是模拟可信观察，不认证 caller bool/签名/IPC。

## 检查与回归证据

- 原 137 个 pytest node ID 全保留，删除/跳过 0；新增 108，当前 245 项通过。既有测试源码未修改，新增测试文件；新 schema 也由既有参数化检查自动覆盖。
- mypy 严格检查 9 个源文件通过，ruff 通过；scope/diff、相对链接和秘密扫描须随本次提交复验，远端 CI 以 PR 最新 HEAD 为准。
- 将 5 个反例输入审查基线的旧 oracle 与修复后的 oracle：额外动作、额外有效 scope、重复动作、重复 scope、无需求提权。旧 oracle 五例均漏报；新 oracle 五例均拒绝。该一次性复现记录在本地 work，正式可持续回归用例位于 test_pr3_remediation.py。
- fixture 只检查协议形状/语义及系统调用观察模型；没有真实 MCP SDK、服务、OAuth/broker、native helper、跨平台竞争/清理或生产安装执行。原 P0 的域契约及租户、事务、证据、删除、恢复测试继续保留。

## 修改文件清单

- `checks/deployment_contracts.py`
- `checks/mcp_wire_contracts.py`
- `contracts/deployment/README.md`
- `contracts/deployment/agent-extension.openapi.json`
- `contracts/deployment/approval.schema.json`
- `contracts/deployment/capabilities.schema.json`
- `contracts/deployment/client-config.schema.json`
- `contracts/deployment/examples/approval.json`
- `contracts/deployment/examples/client-reader.json`
- `contracts/deployment/examples/filesystem-operation.json`
- `contracts/deployment/examples/plan.json`
- `contracts/deployment/examples/synthetic-loop.json`
- `contracts/deployment/filesystem-operation.schema.json`
- `contracts/deployment/filesystem-policy.json`
- `contracts/deployment/mcp-protocols.json`
- `contracts/deployment/mcp-wire-examples.json`
- `contracts/deployment/mcp-wire.schema.json`
- `contracts/deployment/plan.schema.json`
- `contracts/deployment/tools.json`
- `docs/AGENT_INTEGRATION.md`
- `docs/DEPLOYMENT_CONTRACT.md`
- `docs/FILESYSTEM_OPERATIONS.md`
- `docs/MCP_COMPATIBILITY.md`
- `docs/PR3_REMEDIATION_REPORT.md`
- `docs/SELF_HOSTED_ACCEPTANCE.md`
- `docs/SELF_HOSTED_SECURITY.md`
- `docs/adr/008-self-hosting-agent-native.md`
- `docs/adr/009-pr3-remediation.md`
- `docs/adr/README.md`
- `tests/test_pr3_remediation.py`

## 待 Luna / 后续实现确认

1. 对双时代矩阵、首次探测可降级条件、现代错误不得降级及实际支持声明重新独立审查。
2. 对可信锚点假设、平台 native helper 组合、no-replace/清理限制、connect 仅发布新文件的收紧重新审查。
3. 对严格 single-plan 审批与模拟观察的非授权边界重新审查；真实签发者/IPC/签名及提权策略仍待实现。
4. 真实 SDK 四组合、OS 多进程竞态、认证、密钥、备份与删除恢复继续归入 SELF_HOSTED_ACCEPTANCE，不以 CI 代替。

LICENSE、基线 tag、main、PR #2 和其他业务项目必须保持不变。新 HEAD 的 SHA、CI 与同步结果由交付时读取远端报告；本文件不嵌入自身 commit SHA。

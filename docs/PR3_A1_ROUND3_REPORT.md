# PR #3 第三轮 A1 修复报告

Status: design/contracts only; awaiting Luna third independent review. PR #3 remains unmerged.
Reviewed predecessor: `15b12a0f8458d042dc3ef57ac91eb12c14d87624`.

## 修复与边界

1. 响应：固定官方 MCP Schema 源 commit `271ecc9accafdd9b83a3c869fa67c22953b2af80`，原样保留两时代所需定义闭包（modern 31 / legacy 21），随附源 URL、源字节/定义摘要与上游完整许可。输出再叠加产品限制。现代 DiscoverResult/ListToolsResult 必含 resultType/ttlMs/cacheScope 与各自方法字段，首轮 complete/0/private；HTTP private, no-store + Vary: Authorization，代理缓存关闭，跨主体/Space/Grant 不复用。tools/call、ping 已核对为非缓存结果，仍要求 resultType；未扩大 prompts/resources 等功能。legacy 继续旧结构，产品拒绝时代字段混用。
2. fallback：从实际 JSON 结构、关联 ID、官方错误/结果类型、HTTP 状态/media type、探测阶段、模式和受保护时代状态决策，response_kind 只是历史标签。-32603/-32700/-32600、5xx、TLS/网络/超时、认证失败、畸形/重复键/非法 body 或错误 content-type 不触发旧握手。未知端点的 -32601/-32602 以及旧 legacy 缓存也不证明当前时代；要求显式 legacy-only 配置。已识别 modern 不自动降级。仅首次 server/discover 作兼容探测，不使用业务写入、不自动重放。

官方 stdio/HTTP 文档给出的 fallback 建议更宽泛，并非仅允许 -32601/-32602；本项目主动收紧为 fail-closed，代价是部分旧端点需要人工显式配置。官方允许 public/正 TTL，不意味着本产品使用它；第三方接收识别按官方类型，本产品输出 profile 更严。细节和官方链接见 MCP_COMPATIBILITY、ADR-009。

## 测试与复现

- 原有 **245** 个测试 ID 保留，无删除/skip；新增 **204** 个，合计 **449**。
- 新文件 test_pr3_a1_round3.py：199 项（含 108 个机器 fallback 向量及其缓存时代反例）；另外新增 4 个已宣告方法响应 fixture 和 1 个响应 Schema 自校验参数。
- 原 3 个 A1 测试函数覆盖的 22 个旧用例增强：4 个结果改用实际 Schema、6 个现代错误使用结构化响应、12 个降级向量重新解析并纠正其中 5 个不安全预期。为可追溯性显式保留历史 node ID 标签，断言中的预期已更新。
- 对被审查提交原样响应作复现：server/discover 与 tools/list 各自被官方 validator 报两个 required 错误（ttlMs、cacheScope）；修正后通过。负例通过实际 ValidationError/is_valid 失败验证，非仅字段存在断言。
- 本地 pytest 449 项、严格 mypy（11 个源文件）、ruff、JSON Schema/合并 OpenAPI 契约检查通过。扫描、归档复验与远端 CI/HEAD 的最终证据在提交后报告；不得从本文件推定未知 CI 已通过。
- A2/A3 的实现、权限/文件系统机器契约及其 12 个非 A1 测试辅助/测试函数保持不变；受保护 Git blob 与前序一致。根 LICENSE、main、旧标签及 PR #2 不变。

## 尚未执行的运行验收

没有真实 MCP SDK/服务、stdio 子进程、HTTP/SSE adapter、OAuth、代理/SDK 缓存与跨主体泄露验收；没有执行 OS 竞争、Windows 目录持久化、真实审批 broker 或恢复套件。二时代/二传输实际支持列表仍为空。安装助手与记忆客户端分权，P0 事务/证据/删除/恢复/租户规则不变。本报告不是独立审查通过结论。

## 本轮文件清单

- `checks/mcp_responses.py`
- `checks/mcp_wire_contracts.py`
- `contracts/deployment/README.md`
- `contracts/deployment/agent-extension.openapi.json`
- `contracts/deployment/client-config.schema.json`
- `contracts/deployment/mcp-cache-policy.json`
- `contracts/deployment/mcp-fallback-cases.json`
- `contracts/deployment/mcp-protocols.json`
- `contracts/deployment/mcp-response.schema.json`
- `contracts/deployment/mcp-wire-examples.json`
- `contracts/deployment/tools.json`
- `contracts/deployment/upstream/README.md`
- `contracts/deployment/upstream/UPSTREAM_LICENSE.txt`
- `contracts/deployment/upstream/mcp-legacy-results.schema.json`
- `contracts/deployment/upstream/mcp-modern-results.schema.json`
- `contracts/deployment/upstream/provenance.json`
- `docs/AGENT_INTEGRATION.md`
- `docs/DEPLOYMENT_CONTRACT.md`
- `docs/MCP_COMPATIBILITY.md`
- `docs/PR3_A1_ROUND3_REPORT.md`
- `docs/SELF_HOSTED_ACCEPTANCE.md`
- `docs/adr/009-pr3-remediation.md`
- `tests/test_pr3_a1_round3.py`
- `tests/test_pr3_remediation.py`

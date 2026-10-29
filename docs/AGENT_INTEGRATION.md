# AI 原生接入契约 v1

Status: P0 design extension，非已运行服务。统一数据面使用 L2 用例，REST、MCP、SDK 不能绕过事务、证据来源、授权、幂等、CAS、删除栅栏或知识提交序。部署助手控制面仅做安装运维，不能作为普通 Agent 的数据身份。

## 两种独立能力及凭据

| 身份 | 允许 | 默认不授予 |
|---|---|---|
| deployment assistant | 受批准 doctor/plan、owned 路径 init/up、脱敏 status、连接配置 preview | 读取/导出内容、全局 tenant admin、签发数据 Grant、系统管理员 |
| memory client | 服务端绑定 tenant + 允许的 Space 与显式 capability | 安装、升级、恢复、key 管理、服务配置、Grant 管理 |
| owner/security operator | 通过本地受保护管理入口授予/撤销 Grant、轮换 client 凭据 | 不自动把自身凭据交给 AI；tenant admin 仍不自动获个人内容读取 |
| erasure worker | 受控 job 的 generation/闭包能力 | 自由搜索用户内容或越 tenant 操作 |

同一个 AI 可以在不同已授权 session 扮演两种身份，但使用分离凭据、会话、审计和 OS/helper 权限；记忆上下文不注入安装凭据。安装者若有宿主机任意执行/文件访问能力，应用 ACL 不构成其隔离保证，应使用受限 helper 或单独服务用户。

AuthContext 由验证凭据后服务端 Principal/Grant 映射，不信任请求中的 tenant/space、client-config 或 token 自述管理员。初版连接仅绑定一个 tenant 和一个 Space；未来多 Space 必须显式 allowlist。客户端可请求目标但不能声明自己有权限。

## 能力发现与协议版本

REST `GET /v1/tenants/{tenant_id}/spaces/{space_id}/capabilities` 返回经授权过滤的 contract version、工具集、API major、数据 schema、transport、NoModel 标志、索引水位能力及 policy epoch。未认证返回 401；错误 tenant/Space 统一不可见；服务/版本健康 public probe 仅最小 liveness 不泄漏实例细节。

MCP 首发目标 modern `2026-07-28`，兼容目标 legacy `2025-11-25`；实际两者均未运行实现。两时代的 metadata、握手、检测、HTTP headers、错误、会话和工具发现见 [双时代矩阵](MCP_COMPATIBILITY.md)，禁止只替换版本字符串。现代服务必须实现 server/discover，但客户端可不经 discover 直接调用；旧客户端独立 initialize/initialized。tools/list 经授权过滤，调用时重验；首发不宣告 listChanged/subscriptions、sampling、elicitation 等未实现能力。工具目录与 capabilities 只记录设计目标，不能当作实际 SDK 认证。

API major 为 v1，deployment contract 为 1.0、MCP protocol 为日期版本，SDK 软件版本独立。schema 形状沿用 2020-12；新增写字段不默许接受，不支持版本或未知工具返回稳定错误。部署/MCP 扩展本身沿用原事实 DTO；后续 P0-SC 的 Memory.source_blocked 与 Deletion/staging 字段修订以 ADR-010/011 为准。`agent-extension.openapi.json` 为可合并主草案的三个新增端点及一个只读搜索 header 扩展，现有端点不改语义：

- authorized `POST .../payloads`：显式 staging，最大 UTF-8 65536 bytes，不可信 classification 声明、purpose(content/locator)、纯文本；禁任意 URL 拉取。返回 UUID、TTL、用途及服务端 effective_classification/protection_policy_epoch/encrypted，幂等写；大文件/二进制上传另行设计。staged Payload 的 actor/tenant/space/purpose/generation 必须匹配引用用例；仅密文 staging，10 分钟到期调度清孤儿（清理未验证不报成功），已在事务引用的 Payload 不按 staging TTL 擦除。
- authorized `GET .../capabilities`：对该 tenant/Space 与当前主体过滤的能力。

- authorized `POST .../context`：显式 memory IDs、domain、max_bytes≤65536、min_commit_token，返回 context-bundle.schema.json。需要 memory.context、memory.read、evidence.read，v1 只投影当前版本，历史上下文需另行契约扩展与 memory.history；每一对象/来源/正文重验权限。metadata 与可擦除正文同一知识快照对应，读取后在输出前检查当前 auth epoch、删除 generation；变化则中止或重试，不能混入其他版本正文。只返回满足预算的完整片段，不截断为失真事实；超预算标 partial/reason。
- `POST .../search` 保留原输入 body，仅增加可选 `X-Min-Commit-Token` header；SDK/MCP min_commit_token 映射到该 header，非权限凭据。不支持该扩展的旧服务不能宣称支持读己之写 watermark。

Payload staging 独立事务只创建隔离 staging 对象；Event+Evidence 引用仍在 T1 中原子完成，Memory 创建仍另一个合法 T1。写链中任一步失败都不会产生无来源事实；重试各步独立幂等键，孤儿最终清理。NoModel 使用 user_assertion 或经规则验证的 extracted_fact，不能把模型推断自动升级。

## REST、stdio 与 Streamable HTTP

REST 默认 loopback `http://127.0.0.1:8787/v1/...`；远端只走认证 HTTPS。所有访问验证目标 audience、expiry、issuer/本地 secret registry、Principal 状态及 policy epoch。配置中的 yixumemory-memory 是逻辑数据面类别，不是通用 OAuth aud；HTTP MCP 的 resource/audience 必须绑定具体 canonical endpoint URI，禁止仅比较类别字符串就接受跨实例 token。

REST 最小本地个人模式可由本地 owner 创建有界 bearer secret；第三方远端标准 MCP 路径采用下述 OAuth profile，不能把任意 REST token 冒充完整 MCP OAuth 支持。

MCP stdio：客户端启动受信的 `yixumemory-mcp --connection-ref <id>` 薄适配器，通过受限凭据 broker/handle 访问已运行数据服务，不直接打开 DB，不继承安装助手凭据。stdout 只输出换行分隔 UTF-8 JSON-RPC；stderr 脱敏。client config 不含 shell 字符串或 token，参数数组由 adapter 按宿主客户端格式生成；不修改其他 MCP server 项。stdio 会话经 OS 用户/连接 ref 映射有界 Grant，禁止用“本机进程”作为无条件数据权限。

Streamable HTTP：同 `/mcp` 的不同 wire handler 隔离时代。modern 每 POST 校验 params._meta 与 MCP-Protocol-Version/Mcp-Method/Mcp-Name，不一致 400/-32020；无旧 session/GET/DELETE/Last-Event-ID 恢复。legacy 才使用 initialize/initialized 和经认证的可选旧 session、GET/DELETE。现代服务忽略旧 session header，不用它选取旧状态。具体检测/降级以 MCP_COMPATIBILITY 为准，认证或现代错误不得触发静默降级。所有请求校验 Origin/Host、HTTPS 与 resource/audience，禁止 token query/passthrough；取消/断线不等于事务回滚，写重放仍用原幂等键。

标准远端 OAuth 接入目标：protected-resource metadata、授权服务器发现、resource/audience 绑定、PKCE S256、state 与精确 redirect allowlist，scope 映射服务端 Grant；本地部署可配置自托管 issuer，不依赖官方账户或第三方 LLM。认证服务器实现/选择及客户端支持是未来运行验收阻断项；未实现时不宣称可兼容任意 HTTP MCP 客户端。不自动暴露或关闭认证来换兼容。

## 工具、范围与错误

| MCP tool | 最小 capability | 对应 REST operationId |
|---|---|---|
| memory_capabilities | capabilities.read | discoverCapabilities |
| payload_stage | payload.stage | stagePayload |
| events_ingest | event.append | ingestEvent |
| events_get | event.read | getEvent |
| evidence_get | evidence.read | getEvidence |
| memories_create | memory.append | createMemory |
| memories_get | memory.read | getMemory |
| memories_revise | memory.revise | appendRevision |
| memories_history | memory.history | listRevisions |
| memories_search | memory.search | searchMemories |
| memories_context | memory.context + memory.read + evidence.read | hydrateContext |
| memories_erase | memory.erase | eraseMemory |
| deletions_get | deletion.read | getDeletion |

reader 默认只授 capabilities.read、memory.read/search，不自动获得 history/evidence；类似体闭环 writer 明确增授 payload.stage、event.append、memory.append 及所需来源读取。修改需 memory.revise 与 expected_version，删除需 memory.erase 与 expected_version；两者同时要求 idempotency_key。普通 Agent 无 source-erase/Grant/安装/key rotation 工具；敏感来源擦除仍经专门授权的 P0 erasure 协调流程，不从工具目录“缺失”推导数据永不可删除。

MCP 输入显式 tenant_id/space_id、有限参数及幂等键；路径 ID 始终二次授权。内核错误映射：invalid_input → REST 400/422，unauthenticated → 401，not_visible → 404，denied → 403（无存在性差别），conflict → 409，erased → 410（仅仍有权用户），stale_version → 412，precondition_required → 428，rate_limited → 429，unavailable → 503。MCP JSON-RPC unknown method -32601、bad params -32602、internal -32603 用于协议层；业务失败返回 tools/call result `isError:true`，structuredContent 使用 tool-result.schema.json 的 code/retryable/request_id，不伪装正常空结果。HTTP OAuth 401 challenge 在传输层处理，不塞进 200 工具结果冒充认证。

getMemory 和 SearchResult 原 DTO 继续为指针。正文只经新增 hydrateContext 投影，不要求 Agent 直接读取文件/DB；水合不得更新/拼接覆盖原 Revision。工具 structuredContent 是带 commit_token 的脱敏 receipt，资源 DTO 通过独立 resource content block 返回并单独 schema 校验；result_ref 不是免鉴权访问链接。memory/context 输出仍可能携带敏感正文，仅在用户批准的数据接入边界内发送；日志不得记录正文。当前只提供 fixture，真实正文水合、流撤权和外部 Payload store 的读取栅栏均待运行验证。

## 连接配置与上下文

connect 默认 preview 返回 client-config.schema.json 中立 manifest：tenant/Space allowlist、transport、endpoint 或 connection_ref、audience、credential_ref、scopes、API/协议版本；无 token。配置不是权限凭据；server 以授权 registry 为准。用户先通过本地受保护入口授权 client Principal，安装助手只能引用已有受限凭据；不得借 bootstrap admin 代授内容权限。

apply 将中立 manifest 转为特定客户端格式前检查 transport/时代、secret 注入、版本并生成 preview。依 FILESYSTEM_OPERATIONS 初版只能通过可信锚点句柄 no-replace 发布新管理文件；已有客户端配置保持不变，合并改写等待专用 adapter 安全审查，不以原文件 digest 检查后普通覆盖。目标客户端不支持安全 secret broker 时返回 capability unavailable，不打印 token 或退化为把 secret 放在聊天/参数/配置中。SDK 使用同 manifest 权限与错误模型，不强制任何 Agent 框架。

上下文注入是显式客户端策略：用户批准的 Space、真实/fiction 查询域、时间及 token budget；每个片段标 memory/revision/evidence、来源类别、valid time、knowledge token、冲突状态和 `untrusted_data`。从记忆读出的安装指令、管理员自述或 prompt injection 不改变工具/部署权限；不把检索文本放成 system 权限指令。撤权/删除后停止续流，丢弃受控客户端缓存；无法保证抹去已送达第三方模型的历史上下文，发送外部模型需用户及 tenant policy 允许。

## 合成类似体最小闭环

`contracts/deployment/examples/synthetic-loop.json` 是确定性、无模型、无真实秘密的可复核 fixture，包含域 DTO、工具调用和预期结果。本轮 `python -m pytest -q tests/test_self_hosting_contracts.py` 校验输入/输出、引用链、读己之写水位与隔离反例；**不启动 CLI/MCP/服务，不是生产端到端测试**。

未来实机复现步骤：隔离实例 → owner 建立 tenant A/Space A 和无关 B → 给 persona-test 仅 A 的 writer+reader+来源权限 → capabilities 发现 → stage 合成事件文本与 locator → events_ingest 得 Event+Evidence → stage 合成 user_assertion → memories_create 附 Evidence → memories_search 用固定词法 token `synth-cobalt`、domain real、min_commit_token → 得唯一同一 Memory、知识水位不落后 → memories_context 取同版本正文/证据和 untrusted_data 标记 → 验证 B 不可访问、无 memory.erase/安装权限；每步独立幂等键，重复不增版本。索引未追上时权威回退或 503，不能把空列表视为成功。verify 写探针仅在专用 synthetic Space 中显式授权；清理使用另外批准的擦除主体，不给普通 Agent 默认删除权限。

以下为 legacy 固定版本参考；modern 正式版本及差异以 MCP_COMPATIBILITY 的来源矩阵为准：[MCP transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)、[authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)、[lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)、[tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)。具体兼容行为必须跑实际客户端套件；仅对齐文本不算协议实现。

PR #3 修复依据与状态：[ADR-009](adr/009-pr3-remediation.md)。旧协议固定描述由双时代矩阵取代，未改变 L2 业务规则或宣称真实协议支持。

## PR #3 第三轮 A1 约束

modern discover/tools/list 输出经固定官方 Schema 校验，必填 resultType、ttlMs、cacheScope 及方法字段；首轮 complete/0/private，HTTP private, no-store + Vary: Authorization，代理不缓存，跨主体/Space/Grant 不复用能力。tools/call、ping 不强加缓存字段，legacy 保持原结构。能力发现不是授权证明。dual-era 仅初次无副作用 discover 探测，普通 JSON-RPC error、空 body、内部错误、超时和旧 legacy 缓存均不能证明当前时代；无法确认则失败并提示显式 legacy-only。详见 MCP_COMPATIBILITY 与机器契约 mcp-response/mcp-cache-policy/mcp-fallback-cases。没有新增运行支持声明。

P0-SC 合成闭环分别 stage 来源正文、locator 和 Revision 正文三个 Payload，不能让 Memory 正文与独立来源复用同一删除/key 单元；即使文本相同也不默认去重。fixture 仅为预期形状，encrypted=true 不是实际加密执行证据。

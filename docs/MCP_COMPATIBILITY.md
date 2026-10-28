# MCP 双时代 wire 契约（PR #3 A1 修复）

Status: design only / awaiting independent re-review。首发协议**目标**为 modern `2026-07-28`，兼容**目标**为 legacy `2025-11-25`，均覆盖 stdio 与 Streamable HTTP；目前两个时代都没有已实现、已运行或 SDK 认证的声明。不支持更早 HTTP+SSE，也不以协议日期字符串推定已实现功能。部署发现的实际版本列表必须来自已通过运行验收的 adapter；设计矩阵不能直接复制成运行能力。REST DTO 和 13 个业务工具含义保持一致。

## 协议矩阵

| 维度 | legacy 2025-11-25 | modern 2026-07-28 |
|---|---|---|
| 版本/连接开始 | initialize 请求含 protocolVersion、capabilities、clientInfo；服务返回选择版本，客户端检查可接受后发送 notifications/initialized | 每请求 params._meta 必含 io.modelcontextprotocol/protocolVersion 和 io.modelcontextprotocol/clientCapabilities；clientInfo 可选；没有 initialize/initialized 握手 |
| 协商失败 | 服务可返回自身支持的另一 legacy 版本，客户端不支持须结束，不直接带旧状态继续 | 不支持版本用 -32022 UnsupportedProtocolVersionError，data.supported/requested；只能选择确实支持的版本并重新构造正确时代请求 |
| 服务能力 | initialize result；tools/list 经授权过滤 | server/discover 为服务端必实现方法；客户端可以直接调用其他 RPC，不要求每次调用前 discover；tools/list 经授权过滤 |
| 结果 | 旧结果无 resultType；内部统一表示时可视为 complete | result.resultType 必须存在，本产品当前业务仅 complete；DiscoverResult/ListToolsResult 还必含 ttlMs、cacheScope（本产品输出 0/private）；structuredContent 仍是业务 receipt，正文 resource block 另行校验 |
| 状态边界 | stdio 进程连接状态；HTTP 独立 legacy session registry | 无 legacy session 状态；协议元数据不是认证或权限 grant |
| 通知 | 本产品首发不宣告 listChanged，客户端可重新 tools/list | 首发不实现 subscriptions/listen、MRTR、roots/sampling/elicitation，不宣告这些能力；不能用旧 GET 流伪装现代订阅 |

依据：[legacy lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)、[modern versioning](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)、[discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)、[schema](https://modelcontextprotocol.io/specification/2026-07-28/schema)。矩阵描述本产品选择的子集；没有把所有官方可选扩展承诺为首发功能。

## 传输矩阵

| 项目 | legacy stdio / HTTP | modern stdio / HTTP |
|---|---|---|
| stdio framing | 每行一个 UTF-8 JSON-RPC；握手后允许业务请求；initialized 无 id、无 RPC response | framing 相同；每请求携带 _meta，服务不能发送独立反向 RPC；取消使用 notifications/cancelled，不从普通业务请求推导会话 |
| HTTP 入口/headers | initialize 可不带已协商版本；后续请求带 MCP-Protocol-Version=2025-11-25；若服务签发 Mcp-Session-Id，客户端后续携带且服务核验 | 每 POST 带 MCP-Protocol-Version、Mcp-Method；tools/call 另带 Mcp-Name，分别与 body version/method/params.name 一致；header 名大小写不敏感，值大小写敏感 |
| HTTP 流 | POST 及 legacy GET SSE；GET 不启用则 405；legacy session DELETE；可选 Last-Event-ID 仅在实际实现后宣告 | 只有 POST；GET/DELETE 对 modern 为 405；SSE 只属于本次 POST，关闭流为取消；不提供 Last-Event-ID 恢复 |
| Session | legacy HTTP session ID 只定位经过认证的旧会话，绝不是身份凭据；过期 404 后客户端重新握手，不盲重放写 | 不签发/回显/恢复旧式 session；按官方建议忽略旧 Mcp-Session-Id 与 Last-Event-ID，不能因其存在套用 legacy 状态 |
| HTTP 内容 | Accept 同时声明 application/json 和 text/event-stream | 同左；每 POST 单个请求；本产品不实现 modern HTTP client notification 扩展 |
| 元数据矛盾 | 带 modern metadata 的请求不进入 legacy 已初始化 handler；版本与旧 session 不一致拒绝 | 缺失/非法/矛盾的 required mirror header → HTTP 400/-32020 HeaderMismatch；不修正 body、不静默降级；缺必要 body _meta → invalid params |

对 Mcp-Name 采用官方 UTF-8 Base64 sentinel 编码/解码规则；解码后再比较，非法编码、重复或歧义 header 均拒绝。当前工具未声明 x-mcp-header 自定义镜像；未来加入时须按官方规则校验 schema 标注和 Mcp-Param-*，不能随意把客户端参数变成可信路由权限。

依据：[legacy transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)、[modern stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)、[modern HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)。HTTP 代理若依赖镜像 header 做策略，必须同时确认其所属时代及 header/body 校验；不能把 legacy 未验证 header 当租户权限。

## 双时代检测与降级规则

显式 legacy-only 配置从独立入口走旧握手，不先发送 modern 业务调用。modern-only 可以直接调用 RPC；已知现代服务不强制 discover。dual-era **仅在首次识别阶段**用带完整 metadata 的 server/discover 作无业务副作用探测；不使用 tools/call、写入记忆或其他业务操作探测，不自动重放业务请求。ping 可作为已选时代的存活检查，不是本产品的跨时代识别探针。

| 已解析、与探测 ID 关联的结果 | stdio 项目策略 | HTTP 项目策略 |
|---|---|---|
| 完整且经官方 Schema 校验的 DiscoverResult，支持 modern | 保持 modern，记录时代；能力内容仍不构成认证证明 | 仅 HTTP 200 的合法 JSON 最终帧可作该观察 |
| 合法 -32022，含 data.supported/requested | 保持 modern，选择确实支持的版本或报错 | 对应 HTTP 400；不得触发旧握手 |
| 合法 -32020/-32021（后者含 requiredCapabilities） | 保持 modern，修正错误 | 对应 HTTP 400，不能降级绕过 |
| 合法 generic -32601/-32602 | **不足以证明 legacy**，失败并提示显式 legacy-only 配置 | HTTP 400 同样不足以证明 legacy；空/不识别 body 也失败 |
| -32603/-32700/-32600、其他普通 error | 报错，不推定旧时代 | 同左；不能把服务器故障当成协议识别信号 |
| HTTP 404 + 合法 JSON-RPC -32601 | 不适用 | 留在 modern（官方绑定的未知方法语义）；空 body 不作识别 |
| 401/403/429 | stdio 不套 HTTP 状态，凭据失败由 adapter 报错 | 认证/限流失败，不触发降级 |
| 5xx、TLS/网络异常、超时、畸形 JSON、重复键、非法 envelope/body、错误 content-type、ID 不匹配 | 失败，保留诊断；禁止新旧帧猜测 | 同左；重定向或代理 HTML 页面不作旧时代信号 |
| 已识别 modern 后任何异常，或仅有旧 legacy 缓存 | modern 绝不自动降级；旧缓存也不能证明当前端点仍是 legacy | 同左，须显式配置并重新验证认证绑定 |

解析顺序：传输失败/HTTP 状态与 media type → JSON 唯一键、JSON-RPC envelope、匹配 ID → 固定官方结果/error 类型 → 阶段/探针 allowlist、模式与受保护时代状态。HTTP oracle 仅接受 application/json 单个最终响应；text/event-stream 不是合法 JSON body，未来真实 adapter 必须正确解码 SSE、校验最终帧与关联 ID，不能因此推定 legacy。旧的 response_kind 字符串只保留为历史调用标签，完全不作为识别证据；只传 error code/标签也失败。ParsedProbe 是纯测试解析观察，不是可信授权、TLS 验证或真实 transport 的实现。

**官方建议与项目取舍**：官方 [stdio Backward Compatibility](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio) 对首次 discover 收到非现代错误或超时给出更宽泛 fallback 建议；官方 [HTTP Backward Compatibility](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http) 也允许对不识别/空 body 的 HTTP 400 尝试旧初始化。本项目主动不采用该宽泛自动回退，**官方并非只允许 -32601/-32602**。这些码现代服务也会使用，无法安全证明 legacy 时选择失败，要求用户显式 legacy-only 配置，代价是未知旧服务需要一次人工配置、容错与自动发现能力较低。当前 oracle 不会在 dual-era 返回 fresh-legacy-handshake；显式配置是新的独立连接/进程，不搬移旧状态、不重放业务请求。legacy_fallback_allowed=true 只允许进入兼容策略，不是自动回退保证，更不是越过安全门槛的授权。

时代识别状态隔离：stdio 绑定进程 generation、启动配置与可执行文件 digest；HTTP 至少 origin+endpoint+issuer/resource/信任配置及当前认证主体。重启、二进制/配置变化、实例或信任绑定变化后重新识别；不能只按域名跨实例复用，也不接受远端 JSON 自报 cached_era。旧 session、capabilities、请求 ID 和读取缓存不可转入 modern；modern metadata、订阅状态也不可转入 legacy。双时代服务器按 wire 入口选择独立 handler，可共享 L2 业务/权限系统；同端点并行时不能用一个全局 initialized 标志。session ID 的存在不能覆盖 body 的 modern 标记。发现时代从不改变认证、tenant/Space、幂等键、CAS、证据和删除栅栏。

## 响应与缓存契约

mcp-response.schema.json 对输出采用两层校验：固定官方 Schema 的原样 $defs（见 upstream/provenance.json 的源 URL、commit、SHA-256 和上游许可证），再叠加本产品限制。DiscoverResult 必填 resultType/ttlMs/cacheScope/capabilities/supportedVersions，ListToolsResult 必填 resultType/ttlMs/cacheScope/tools；ttlMs 官方为非负整数，cacheScope 官方可 private/public。首轮本产品对这两个响应输出 resultType=complete、ttlMs=0、cacheScope=private，不输出 public 或正 TTL。非缓存的 tools/call、ping 仍要求 modern resultType，产品输出拒绝额外 ttlMs/cacheScope 修饰。modern 输出也拒绝根层 protocolVersion/serverInfo 旧握手字段；合法业务嵌套数据不受该限制。没有宣告 prompts、resources 等可选方法，不扩大其支持范围。legacy initialize/tools/list/tools/call/ping 按旧版官方定义校验，产品输出禁止混入 modern 字段；这是本产品防混用约束，并非声称 legacy 官方 Schema 禁止所有额外属性。

mcp-cache-policy.json 指定：HTTP discover/tools/list 无论成功或失败均设置 Cache-Control: private, no-store、Vary: Authorization，反向代理禁用缓存（Vary 本身不是隔离保证），也不缓存错误或未经授权结果。完整授权上下文必须包括 Principal、tenant/Space、resource/audience、凭据/Grant 版本及策略快照；不可跨主体复用能力、工具列表或过期结果。撤权、轮换、切换 Space 或策略变化时丢弃旧结果并重新授权获取。ttlMs=0 也禁止 SDK 保留能力结果供后续业务决策，时代识别标记不是能力缓存；每个工具调用仍由服务端重新授权。stdio 遵循同样的主体隔离及零 TTL。安装管理凭据仍不得交给记忆客户端。

接收第三方 modern discover 的解析器按官方允许的 public/正 TTL 识别其协议时代；不能把本产品的输出偏好误当成官方互通限制，但本项目客户端对授权相关发现仍不复用缓存，不因第三方 public 指示绕过隔离。测试验证官方允许与产品输出限制的区别。HTTP/proxy/SDK 真实缓存行为和跨主体泄露测试仍是运行验收，Schema 不能证明这些 header 已被服务或代理执行。

## 错误与授权

modern protocol errors：-32020 HeaderMismatch、-32021 MissingRequiredClientCapabilityError、-32022 UnsupportedProtocolVersionError，HTTP 均 400；未知 RPC 为 -32601，modern HTTP 404；无效参数 -32602。不支持的 modern 请求应列出适当版本诊断；initialize 不是 modern 握手替代品。请求 metadata 声明 legacy 日期但仍走现代帧时，本产品报 invalid params，要求另建 legacy 握手，绝不静默解释为已初始化。

legacy 不支持版本时的应答选择与客户端终止检查由旧握手规则控制；没有把 modern -32022 当成 legacy 标准错误。协议错误在领域用例之前处理；tools/call 的业务失败仍走 isError/receipt，不混成版本错误。

两时代 HTTP 均执行 OAuth resource/audience/issuer/expiry 校验与服务端 Principal/Grant；stdio 均从受保护环境或 broker 获取有界凭据，不以 _meta.clientInfo 自述身份授予权限。server/discover 返回自身信息也不是身份认证证据，tenant 内容不能放进公开可缓存 discovery。YixuMemory 的 discovery 默认同样要求认证并按主体过滤，敏感能力结果 private/no-store，Grant 变化后重新获取。依据：[modern authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)。

## 可复现契约与运行门槛

mcp-protocols.json 是设计矩阵，mcp-wire.schema.json 是本产品请求 fixture 子集，mcp-wire-examples.json 提供两时代请求与结果。checks/mcp_responses.py 离线使用固定官方类型及产品输出 Schema；mcp-fallback-cases.json 与 tests/test_pr3_a1_round3.py 验证错误分类、缺字段与保守 fallback。checks/mcp_wire_contracts.py 只计算规范化模型决策，不执行网络、握手、OAuth、会话注册、SDK 或业务调用。测试包括直接 modern 业务请求不经 discover、legacy 握手/initialized、错误版本、header/body/name/method 不同、缺 metadata、现代错误不得降级及旧 session 不得污染 modern。

未来必须用真实 SDK/服务对两传输和两时代各自执行初始化或逐请求调用、SSE、错误矩阵、认证、撤权、幂等写和重连。实际支持版本仍为空，直至该运行套件通过；不得以这些 fixture 将任一时代标为已支持。

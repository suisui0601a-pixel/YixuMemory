# MCP 双时代 wire 契约（PR #3 A1 修复）

Status: design only / awaiting independent re-review。首发协议**目标**为 modern `2026-07-28`，兼容**目标**为 legacy `2025-11-25`，均覆盖 stdio 与 Streamable HTTP；目前两个时代都没有已实现、已运行或 SDK 认证的声明。不支持更早 HTTP+SSE，也不以协议日期字符串推定已实现功能。部署发现的实际版本列表必须来自已通过运行验收的 adapter；设计矩阵不能直接复制成运行能力。REST DTO 和 13 个业务工具含义保持一致。

## 协议矩阵

| 维度 | legacy 2025-11-25 | modern 2026-07-28 |
|---|---|---|
| 版本/连接开始 | initialize 请求含 protocolVersion、capabilities、clientInfo；服务返回选择版本，客户端检查可接受后发送 notifications/initialized | 每请求 params._meta 必含 io.modelcontextprotocol/protocolVersion 和 io.modelcontextprotocol/clientCapabilities；clientInfo 可选；没有 initialize/initialized 握手 |
| 协商失败 | 服务可返回自身支持的另一 legacy 版本，客户端不支持须结束，不直接带旧状态继续 | 不支持版本用 -32022 UnsupportedProtocolVersionError，data.supported/requested；只能选择确实支持的版本并重新构造正确时代请求 |
| 服务能力 | initialize result；tools/list 经授权过滤 | server/discover 为服务端必实现方法；客户端可以直接调用其他 RPC，不要求每次调用前 discover；tools/list 经授权过滤 |
| 结果 | 旧结果无 resultType；内部统一表示时可视为 complete | result.resultType 必须存在，本产品当前业务仅 complete；structuredContent 仍是业务 receipt，正文 resource block 另行校验 |
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

显式 legacy-only 只走旧握手。modern-only 可直接发送 RPC；对已知现代服务不强制 discover。为了首次识别未知旧服务，dual-era 的 stdio 建议先发带完整现代 metadata 的 server/discover 探针；HTTP 首次检测使用无业务副作用的 discover/ping，不以 memories_create 等写调用试探时代。

| 探测结果 | stdio dual-era | HTTP dual-era |
|---|---|---|
| DiscoverResult | 缓存 modern，选择共同版本 | 同左 |
| -32022 | 已识别 modern，选择 supported 中的兼容版本或报错 | HTTP 400 的同类 error 不代表旧服务器 |
| -32020/-32021 | 留在 modern 修正请求/能力，不能旧握手绕过 | HTTP 400 也不得降级 |
| generic -32601/-32602 或超时 | 只在尚未识别时代的 discover 探针中可进入新 legacy 握手；超时重启受限子进程以隔离迟到结果 | 只对首次检测的 HTTP 400 且 body 不是可识别 modern error，允许新 legacy 握手 |
| HTTP 404 + JSON-RPC -32601 | 不适用 | 留在 modern，表示方法不存在；404 空 body 是路径/配置问题，本产品不回退已废弃 HTTP+SSE |
| 401/403/429、TLS/网络异常 | 修复认证或报错，不能借降级获取权限 | 同左；不得将认证失败解释成协议时代不兼容 |
| 已缓存 modern 后的异常 | 不自动切换 legacy；失败或经显式配置重新探测 | 同左 |

缓存隔离：stdio 绑定进程 generation；HTTP 至少 origin+endpoint+issuer/信任配置，不能只按域名跨实例复用。旧 session、capabilities、请求 ID 和读取缓存不可转入 modern；modern metadata、订阅状态也不可转入 legacy。双时代服务器按 wire 入口选择独立 handler，可共享 L2 业务/权限系统；同端点并行时不能用一个全局 initialized 标志。session ID 的存在不能覆盖 body 的 modern 标记。发现时代从不改变认证、tenant/Space、幂等键、CAS、证据和删除栅栏。

## 错误与授权

modern protocol errors：-32020 HeaderMismatch、-32021 MissingRequiredClientCapabilityError、-32022 UnsupportedProtocolVersionError，HTTP 均 400；未知 RPC 为 -32601，modern HTTP 404；无效参数 -32602。不支持的 modern 请求应列出适当版本诊断；initialize 不是 modern 握手替代品。请求 metadata 声明 legacy 日期但仍走现代帧时，本产品报 invalid params，要求另建 legacy 握手，绝不静默解释为已初始化。

legacy 不支持版本时的应答选择与客户端终止检查由旧握手规则控制；没有把 modern -32022 当成 legacy 标准错误。协议错误在领域用例之前处理；tools/call 的业务失败仍走 isError/receipt，不混成版本错误。

两时代 HTTP 均执行 OAuth resource/audience/issuer/expiry 校验与服务端 Principal/Grant；stdio 均从受保护环境或 broker 获取有界凭据，不以 _meta.clientInfo 自述身份授予权限。server/discover 返回自身信息也不是身份认证证据，tenant 内容不能放进公开可缓存 discovery。YixuMemory 的 discovery 默认同样要求认证并按主体过滤，敏感能力结果 private/no-store，Grant 变化后重新获取。依据：[modern authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)。

## 可复现契约与运行门槛

mcp-protocols.json 是设计矩阵，mcp-wire.schema.json 是本产品请求 fixture 子集，mcp-wire-examples.json 提供两时代请求与结果。checks/mcp_wire_contracts.py 只计算规范化模型决策，不执行网络、握手、OAuth、会话注册、SDK 或业务调用。测试包括直接 modern 业务请求不经 discover、legacy 握手/initialized、错误版本、header/body/name/method 不同、缺 metadata、现代错误不得降级及旧 session 不得污染 modern。

未来必须用真实 SDK/服务对两传输和两时代各自执行初始化或逐请求调用、SSE、错误矩阵、认证、撤权、幂等写和重连。实际支持版本仍为空，直至该运行套件通过；不得以这些 fixture 将任一时代标为已支持。

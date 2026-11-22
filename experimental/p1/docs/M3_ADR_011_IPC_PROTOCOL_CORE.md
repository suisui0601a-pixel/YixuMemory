# ADR-M3-011 IPC 协议核心与会话状态机

基线 `ced4e16ae5d26f5113aee5cd9832326fe72fda09`。接受 R3 实现方向。授权与发布状态按实际验收分别记录。

将规范帧、认证契约、会话与调度放在独立 `yixu-ipc` crate。现有 Ports、Application、Auth、Memory、Conflict、Persistence、Controlled Release、W4 Profile 9 不增加对 IPC 的依赖。保持事务、CAS、幂等、knowledge_seq、Outbox 及现有撤权语义。将来由受信 Broker 接管私有装配，串行接收安全事件，在业务事务中重新获取实时 Grant 和业务 AuthContext。控制请求描述符仅携带标识和 epoch，无写入、终态签发、解除栅栏或解封权限。

采取固定二进制 v1 和精确版本匹配。对于当前只有一个版本的协议，较宽的版本协商增加降级和别名风险。未来版本须定义独立的选择规则与签名输入，并在升级前验收降级矩阵。采用已有 SHA-256 为规范请求摘要。合成证明使用现有 HMAC 依赖与明确的测试角色分离。生产证明的 32 字节槽预留给受认证传输导出的证明机制。实际机制仍须按 ADR-M3-008 选型验收。

选择每 realm 单调度所有者、保留有界去重历史、全局一个在途操作和 round-robin。背压保留安全优先级和可预测资源使用。跨进程唯一所有者、持久业务重试、取消结算和恢复证据由后续 Broker 与受信适配器处理。其他备选包括共享并行工作池、自动排出旧请求 ID 与跨连接复用认证。它们需要额外的隔离或持久状态，首版不采用。

核心普通依赖图仅到 SHA-256 及其传递依赖。合成证明和分块传输在未发布 test-support crate，核心仅在测试图中引用。外部可实现描述性认证 trait，私有 Core 装配不接受外部注入。默认入口返回 CapabilityUnavailable。Native、Profile 9、NoContentRuntime、现有 schema 和迁移无变化。

接口及失败语义见 [R3 线协议与调度契约](M3_W4A2_R3_IPC_PROTOCOL.md)。验收要求包括真实的外部编译拒绝、all-features 图、旧运行时回归、状态与故障矩阵。真实 Windows 服务身份与 IPC 通道、生产 Broker、A3、R12 另有独立关卡。

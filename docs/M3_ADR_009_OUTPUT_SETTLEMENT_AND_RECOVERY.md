# ADR-M3-009 输出终态、持久结算及恢复准入

2026-11-10。设计定案，运行验收待定。接续 ADR-M3-007 与 W4 契约。RecoveryBlocked 是缺省状态。

| 证据 | 签发者与绑定 | 能证明的范围 |
|---|---|---|
| NativeTerminalEvidence | 匹配的 Native completion 收割器。绑定 realm、OperationId、ConnectionId、Attempt、Incarnation、写入长度、状态与 Exact/Range | 单次 I/O 终结及可信计数区间。仅私有构造，禁止请求、反序列化或模拟入口签发真实证据。 |
| PersistedSettlementReceipt | SQLite 短事务。重新校验操作身份与 phase，原子追加证据、结果和审计，确定安全配额结算 | 真实结果已经持久记录。回复丢失只查询回执，不重放正文。 |
| ProcessExitObservation | 可信生命周期管理器，持有从创建或可信交接取得的稳定 process HANDLE | 已核对进程的退出。保留原生 I/O 退出语义，不能推导具体操作结果、计数或外部句柄状态。 |
| OwnershipAdmissionEvidence | 受信部署及恢复检查。绑定服务身份、安装完整性、资源 ACL、启动实例、realm 与旧 owner 交接 | 唯一输出所有者的前提。一次无句柄扫描不构成证明。 |
| RecoveryAdmissionProof | 可信 Broker 恢复校验器。绑定当前部署、realm、incarnation、schema、授权版本、key/generation、撤销与安全状态、结算和预算 | 当前一次准入。历史 Ready 行、旧许可和普通请求不可复用。 |

输出排序权覆盖可能交付字节的整个区间。撤权、来源失效、删除、授予、当前 Revision、Key、Copy 或其他读取资格变更同用排序器。Closing 停止新准入。输出已开始则请求取消，等待真实终态、结算和连接预算处理，再允许 EffectiveFence。SQLite 写事务不跨 DPAPI、文件或管道 I/O。多个 Closing 独立结算，互不解除。

Timeout、CancelIoEx 返回、ERROR_NOT_FOUND、客户端断开、CloseHandle、PID 消失或实例锁释放都不能签发终态或 Ready。仅在匹配原生终态后才释放可能被内核引用的缓冲及 OVERLAPPED。错误终态计数未知用 Range(0,B)，区分 Unknown + NoPending 与 Unknown + Unsettled。Exact(B) 的 Accepted 表示本次写完成及结果已持久，不表示客户端已读。客户端保留的明文不在撤回范围。

原生已终态但 SQLite 提交失败时保留证据与逻辑阻断。可以仅重试匹配结算事务。若进程退出丢失未持久证据，保持 Unknown 和 RecoveryBlocked。输出副本生命周期或预算不可对账同样阻断。禁止 PID-only、lock-only、lease-only 接管、超时伪造终态或自动重发 Unknown。

首版只允许两类 Ready。

1. 全新可信 realm。安装权威确认未复用旧身份、旧目录、旧 DB 或旧撤销历史，身份和存储边界通过检查。
2. 证据完整的干净停止恢复。先关闭新准入，逐操作收割并持久结算，处理所有 Closing 和连接预算，记录绑定旧实例和当前安全状态的停机证明。可信生命周期证实旧 owner 已退出且不能继续写。重启核验当前身份、Profile、Schema 和撤销全集，生成新 incarnation 再签发。缺少任一证据则阻断。

仅凭启动探针、NTFS 锁、SCM Running 或历史标记均不能解除 RecoveryBlocked。拒绝已知旧副本、错配 Profile 和不确定恢复。Local 不承诺检测整机一致快照回滚。Protected 需独立恢复域和排序权威，W4 协议保留为后续验收。

Profile 9 的不可变绑定、phase、唯一活动行与证据外键继续作为合成基线。持久化可验证结算、部署绑定和干净停止证明需要后续新 Profile 或显式受审扩展。当前没有 Ready 签发器，不修改 Profile 9 或自动升级 Profile 8/9。W3 历史合成启动逻辑不作为服务恢复入口。

失败测试包括取消与正常完成交错、completion 后持久化失败、回复丢失、暂停旧实例、伪造历史 Ready、错配 generation、同名管道、多个 Closing、连接预算无法退账。验收必须给出真实事件顺序及数据库证据。仅以不干净恢复换取可用性会丢失证据链，因此延后该能力。见 [S1 实验手册](M3_W4A2_R2_S1_PLAN.md)。

依据 [CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex)、[TerminateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)。

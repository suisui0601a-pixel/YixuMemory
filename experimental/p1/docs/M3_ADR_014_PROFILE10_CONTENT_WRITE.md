# ADR-M3-014 Profile 10 内部正文创建与恢复

2026-11-19，R6-B1。本轮承接 R5 `6e4139d63362b8a69011b277250848270a45037e` 和 R6 未提交的原语提取。

## 决定

在 SQLite crate 现有 Windows 私有测试装配中建立 fresh-only Profile 10。Application 源文件 `internal_content.rs` 由该装配通过 source include 编译，正式 Application crate 没有该模块或 feature。这样真实 UTF-8 请求经过 Application 的写入契约，同时保留现有生产 NoContentRuntime 的关闭状态。

Profile 10 的业务图、授权投影、写入意图、安全事件、Receipt 和 Outbox 共用一个 SQLite 数据库。新建必须检查根目录布局及空的密文、密钥命名空间。application_id 为 1498952246，user_version 为 10，编译的完整 DDL 摘要与实际 sqlite_schema、完整性及外键同时核对。安装、Realm、启动实例和目录对象身份独立持久化。已有 Profile 的打开器不认识这个新格式，Profile 10 也没有导入或升级旧库的分支。

R4 的授权快照查询、Closing 判断及 EffectiveFence 顺序转换提取到私有共享源码。R5 的规范化依赖图摘要也由旧 Graph 和 Profile 10 使用同一实现。新 Profile 为真实对象准备自己的关系表和依赖清单。B1 复用这些安全门及约束，完整的 R5 多类命令、删除闭包、R3 会话认证及原生输出结算继续由各自原有测试覆盖。

## 内容与密码学

Application 接受 `&str`，正文长度限定为 1 至 4096 UTF-8 字节，超长请求明确拒绝。每次新 Payload 都由 W2 创建独立的 OS 随机 DEK，用 CurrentUser DPAPI 封装，再由已有 R6 XChaCha20-Poly1305 实现加密正文。W2 包装内部包含 DEK 和校验用的绑定上下文，不包含正文。每次加密单独取得 OS 随机 nonce。

AAD 沿用现有规范编码，其中 Realm、Tenant、Space、Revision owner、Payload、Key ID、Generation 和长度为显式字段。protection digest 以固定域标签绑定 Profile 10 的格式版本、安装 ID 及 Memory ID。封装与密文使用完全相同的 PayloadBinding。持久化存储中的包装摘要、密文摘要、文件对象身份和绑定都必须匹配。未知或损坏对象不会触发密钥替代或自动重新加密。

DPAPI、AEAD 和 W1 I/O 位于 SQLite 写事务之外。SQLite 使用现有标准 VFS 写入元数据，位于锚定祖先目录内。W1 管理密文、密钥包装、独占文件发布和枚举。这里未提供生产级 SQLite HANDLE VFS、服务账户隔离或 ACL 配置验收。

## 事务与恢复

Tstage 在真实授权、证据依赖和创建 CAS 校验后，持久化完整请求身份和随机对象 ID。请求 ID 以 scope 与 Principal 隔离。相同 ID 的已提交重试必须同时匹配元数据及解密后的实际字节，返回原始元数据回执。未完成请求保持阻断，不生成第二份内容。

Tseal 先消费单次操作状态，完成包装、AEAD、W1 no-replace 发布及独立读回验证，再用短事务登记 publication 收据。T1 重新读取 OS 时间、当前 Grant、epoch、Closing、依赖世代和当前版本 CAS，在单一事务写入业务图、当前指针、安全事件的 EffectiveFence 与 Applied、Knowledge Sequence、Key 元数据、Receipt、Outbox 和完成状态。该步骤任意失败都整体回滚。T1 已提交而响应丢失时，只能确认已持久化的回执，不推断调用方已经收到。

正常停止需要所有意图完成、文件及密码学收据可验证、所有安全事件已结算。停机回执绑定安装、Realm、incarnation、启动 nonce、epoch、Knowledge Sequence、schema 摘要和整个相关元数据账本摘要。受控父进程保存实际 Child HANDLE，通过该子进程继承的 stdout 接收与随机启动 nonce 对应的元数据确认，等待真实退出后才构造私有退出证明。证明没有接受任意 Child 加上调用方 incarnation 的构造入口。

新进程先只对账，必须同时验证停机回执、退出证明及当前状态。最后在写事务中重新核对账本并消费停机回执，记录新 incarnation。非正常终止、Unknown、未完成意图、存储对象缺失、损坏或未登记对象均维持 RecoveryBlocked。已经合法撤权或过期但持久结算完整的记录可保留并正常停机，后续请求仍需重新获得授权。

## 取舍与边界

直接改写 Profile 9 会改变旧原生终态语义，因此使用独立格式。授权库与业务库分开提交会留下可见性窗口，因此使用单库原子 T1。凭锁释放、PID、文件存在或 clean 布尔值直接恢复，会遗漏未决操作，因此要求独立停机回执、启动通道关联、实际进程退出和内容验证共同成立。

本地安装内的状态均处于同一回滚域。OS 时间可被管理员或系统调整，当前实现没有独立可信时间或整机快照回滚检测。DPAPI 不隔离同一 Windows 用户的恶意进程。内存缓冲区尽量锁页并清零，但不承诺阻止调试器、休眠、转储、交换或密码库内部拷贝。

B2/B3 仍需受控读取、历史 Revision、修订、撤销／删除闭包及相关恢复验收。B1 的内部解密用于封装验证和测试断言，不是正文释放接口。

B1 的进程证明装配只验收一次受控子进程停止后的恢复与继续写入。连续多轮 owner 交接和重复停机／恢复链没有在本轮测试，不能据此宣称通用进程生命周期管理已经完成。

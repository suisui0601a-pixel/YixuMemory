# ADR-M3-010 DPAPI 身份迁移与重封装

2026-11-10。设计定案，迁移入口暂缺。W2 的当前用户作用域保护不随文件 ACL、服务注册或 Service SID 改变。服务身份首次启用优先使用新 realm，现有合成库保持原有 Profile，不自动导入。

未来显式迁移须由可信管理路径批准。单次 MigrationId 绑定源和目标 TokenUser、服务 SID、Profile、realm、完整 manifest、当前授权 epoch、目标包装版本及期限。启动先冻结源和目标的输出与写入，排空并结算旧操作。验证最新登记、来源和撤销全集。拒绝缺失或不可信的历史、来源失效、已经撤销或世代错配的 DEK。保留全部 KeyId、Generation 和 tombstone，绝不重新启用旧项。

可信迁移执行器在经过验证的源上下文中解封，在目标上下文中用用户作用域、UI_FORBIDDEN 重新封装。身份切换、Profile 加载和失败清理须独立验收。可行性候选是在受信执行器内临时采用绑定源身份的上下文，然后恢复目标身份。拒绝模拟、恢复身份或封装失败，不以常驻提升、Machine Scope 或普通应用转发裸 DEK 弥补。DEK 仅驻留受控缓冲并按既有 W2 规则清理。普通客户端无导出接口。

| 状态 | 允许的进展与崩溃处理 |
|---|---|
| Frozen → Prepared | 双方持续阻断。持久 manifest 记录前后保护身份和撤销状态。授权或 manifest 不匹配则停。 |
| Prepared → TargetWrapped | no-replace 发布新包装，保留逻辑 KeyId、SealGeneration 和密文 AAD。增加包装版本与目标 vault incarnation，记录旧到新绑定。不覆盖 W2 registry 或旧包装。缺失、孤儿、冲突哈希先对账。 |
| TargetWrapped → TargetVerified | 实际目标身份验证解封、范围和哈希，原子登记目标包装。目标仍关闭。重复操作必须匹配 MigrationId 和包装回执。 |
| TargetVerified → SourceRetired | 持久标记原权威退役并核验源退出，保留 tombstone 和审计。跨库未知结果保持双方阻断。 |
| SourceRetired → CutoverCommitted | 目标重新核验源退役、当前授权、对象和撤销全集，再按准入契约启用。回复丢失只查持久状态。 |

同一 DEK 的重封装不会销毁旧包装，也无法阻止旧 Profile 解封已保留的 wrapper。Local 拒绝已知旧副本，完整一致快照可绕过本地撤销；不能把迁移承诺写成密码学擦除。Protected 也无法收回已外泄的 DEK。若未来另行使用新 KeyId 和密文世代，需要独立复核历史披露与删除语义。

兼容策略为新 Profile 或显式受审导入。保留 W2 AAD 原语与 DPAPI flags，不修改 Profile 7 或 Profile 9。在合成服务 Profile 中逐步验证跨身份错误、无法加载 Profile、崩溃恢复、重复导入、撤销历史缺失、旧包装仍可解封和目标不可用。任何一项无法验证则迁移仍不可用。现阶段只准备实验，本 ADR 无迁移实现。

依据 [CryptProtectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)、[W2 KeyVault ADR](M3_ADR_005_LOCAL_KEYVAULT.md)。

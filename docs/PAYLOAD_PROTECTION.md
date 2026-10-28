# P0-SC：Payload 分类、加密与失败拒绝

Status: Proposal / design contracts only，待 Luna 独立审查 R14。沿用现有 Policy、KeyVault、PayloadStore、UoW 和 NoModel Ports，不提供生产加密实现。

## 服务端分类

公开 PayloadInput.classification 保留 synthetic/personal/sensitive 兼容字段名，但它是 **declared_classification，不可信声明**。不得由它选择明文 adapter、绕过 key backend、降低已有保护或批准外发。synthetic 只代表数据属性，既非安全等级，也非生产免加密凭据。

有效安全等级只有 personal ≤ sensitive。服务端 Policy 计算 Space 下限、可信来源登记、Event/Evidence/全部关联证据、派生/定位/模型产物及之前保护等级的 join，再纳入客户端声明的上调要求。未知、缺失、不可信或无法遍历的输入按 sensitive；无法证明完整关系/权限时直接拒绝绑定，而非虚构来源。客户端 synthetic/未知声明不能降低等级，本轮保守映射为敏感保护；Space 下限不是上限。可用确定性规则/人工受信配置实现，无需模型。

Event 的有效分类从全部关联 Payload 得到；Evidence join Event、正文及 locator；Revision join 正文、所有 relation 的 Evidence、provenance 与派生来源。派生 index/cache/export/摘要等级不低于任一输入。绑定或修订可能提升保护，绝不降低旧 Payload/历史保护。不可变逻辑 Event/Revision 不被原地改写：ProtectionBinding 的单调服务端投影/策略 epoch 与追加保护事件记录当前等级，旧快照分类不作为权限或加密依据。跨对象关联与投影更新在现有 T1 中完成；缺 Projection/保护记录按未知敏感并拒绝不安全绑定，不能按默认 synthetic 导入。JSON resource 原有事实类别 raw_event/model_inference/fictional_setting 与安全分类正交。

## 全生产内容加密边界

P1 生产实例对**所有内容型 Payload**（包括 locator、provenance、索引/导出投影与合成测试内容）均在持久化前加密，personal 也不能明文落盘。必须有通过运行验收的安全 KeyVault backend。NoModel 保留：本机离线安全密钥后端是允许方案，无需第三方 LLM/联网；没有可用安全密钥后端只能提供无内容诊断，拒绝内容写入。

数据库、SQLite WAL/rollback journal/TEMP/临时排序文件、持久索引元数据、日志、trace、审计、错误、队列、幂等记录和备份都不得保存可恢复内容明文。locator/URL、查询词、embedding、词项、摘要、错误中的原文也是内容。只保留最小 opaque ID、generation、policy epoch、无内容状态；元数据仍有再识别风险，沿用最小权限/保留期。内存中经授权解密用于返回或计算是必要能力，不等于允许落盘。

不向 SQLite SQL 参数或 FTS5 表传入真实正文/词项；contentless FTS 不等于词项保密。原生 SQL FTS 不作为 P1 安全默认路径。NoModel 的确定性检索可先在有界授权候选上于受控内存解密作词法匹配，按既有 partial/水位/503 语义报告未穷尽；或使用经独立验收的加密投影 adapter，不能悄悄以明文索引换取性能。未测量吞吐。SQLite 临时文件种类见[官方说明](https://www.sqlite.org/tempfiles.html)，FTS 词项索引见[官方结构文档](https://www.sqlite.org/fts5.html#the_fts_index_idx_and_data_tables)；仅配置 temp_store、VACUUM 或磁盘加密不能代替逐 Payload 可擦除 key 与应用内容边界。

## KeyVault / PayloadStore Port 契约

| 操作 | 输入/输出与失败要求 |
|---|---|
| readiness / allocate / lookup_intent | 服务端 AuthContext、tenant/Space、随机 Payload ID、purpose/generation、有效保护上下文；以 Tstage 持久 key_intent_id 幂等分配并支持按 intent 查询，响应丢失可恢复同一 key_ref；检查安全 backend/独立 key 生命周期；无 backend/key 不可用 → key_backend_unavailable，503，无内容持久写 |
| seal | 使用审查通过库的 AEAD profile（不自创算法），绑定 AAD：tenant/Space/Payload/purpose/generation/封装时保护上下文版本；backend 管理正确 nonce/key 使用；返回密文及 opaque key_ref/profile，失败 → encryption_failed，503 |
| open | 先当前鉴权、有效分类、删除/撤权/key 栅栏，再校验 AAD/auth tag；失败不返回部分明文、不尝试旧 key 或低保护 adapter |
| revoke / verify | 独立撤销 Payload key 及所有可恢复副本，返回有 generation 的受保护回执；失败/无证明 → blocked；旧备份或轮换不能复活 revoked key |
| publish / erase | PayloadStore 仅接受密文对象与受保护元数据，原子无覆盖发布；擦除按 generation，与现有文件系统安全 helper 契约一致 |

AEAD 的属性参考 [NIST SP 800-38D](https://csrc.nist.gov/pubs/sp/800/38/d/final)，此处不指定未经实机验收的实现/供应商，不宣称已验证密码安全。AAD 不能含原文或个人定位。每个可独立擦除单元有独立 key_ref；禁止同 key 横跨不兼容删除单元、跨 Space 去重或把可恢复 DEK/其可逆包装写入普通数据库备份。KeyVault 管理 key、保护和撤销登记，DB 只保存 opaque 引用；master key 长期存在不等于 DEK 可被彻底撤销。所有 key 备份仍受删除回执规则约束。

只接受服务端装配的安全 backend 能力，不接受 JSON encrypted=true/key_backend_safe=true 作为事实。payload.schema.json 是生产持久元数据形状，encrypted=true 不是加密证明；密钥健康、密文认证和存储实际字节必须由真实 adapter 验证。公开 staging response 的 effective_classification/encrypted/policy epoch 是受保护的服务端观察，不是客户端输入能力。

## Staging、T1、幂等与失败矩阵

1. 认证/限额/Space 策略/当前 key readiness。原文只在受控请求内存；禁用请求体日志、代理 body 落盘缓冲、multipart 明文临时文件、崩溃转储及未受控 swap/spill。生产部署无法提供这些必要边界时拒绝启用内容路径。已完全控制服务进程/同安全主体的攻击者可读 RAM，不在“可抵御所有宿主攻击”的承诺内；托管临时区/日志仍属于应用责任。
2. Tstage 以现有 UoW 持久登记随机 staging ID、generation、TTL、key 意图及补偿/cleanup journal（无正文/内容摘要）。登记失败不得创建外部对象。backend 若不能按 intent 协调丢失响应/重复分配及孤儿清理，readiness 拒绝该生产内容路径，不能靠盲重试生成未知 key。外部 key 分配、seal 和密文发布均在 T1 外；只允许密文进入 staging，seal 失败也不先写明文“临时过渡”。Tseal 记录已认证密文的 server protection 元数据，201 仅在成功后返回。
3. T1 绑定 staging 时重新鉴权、遍历来源/证据/派生、join 当前保护和删除栅栏；staging 不代表已批准事实。AAD 的 seal_context_id 指向不可变封装时上下文；当前 protection_policy_epoch 是独立服务端投影，open 先按当前策略授权再使用原 seal context 校验，不能用旧 AAD 版本当授权。策略提升可在原 profile 仍满足当前下限时只上调受信保护投影，不改写 seal context；若作用域/purpose/generation/封装上下文或必要 profile 变化，重新 seal 到新 generation；失败则不提交任何 Event/Evidence/Revision/current/outbox 部分状态。禁止为提升分类原地解密写临时明文。
4. COMMIT 后关联可读；rollback/取消/超时/孤儿立即失去客户端水合能力，密文与孤儿 key 按既有 outbox/补偿登记清理。TTL 只是清理调度上限策略，不是“清理已成功”的证明；key revoke/blob erase 失败保留受限 journal，状态 blocked 并告警，重试到有效回执前不删登记。即使崩溃在 key 创建或密文发布后，预先登记的补偿意图可识别对象。不得声称跨 DB/KeyVault 原子事务。
5. 重试/幂等缓存从不携带正文、密钥或旧“已分类所以跳过”标志。每次重用 staging/结果引用都重新授权、检查当前 epoch、删除状态和保护 join；当前安全条件不足则 503/409，不能靠旧 synthetic/personal 结果声明本次新写成功。已提交事实不重复创建，原 30 天期限不变。

| 故障点 | 必须结果 |
|---|---|
| 无安全 backend、key 不可用、seal/auth tag 失败 | 拒绝内容持久写；无明文 staging，不能降级；补偿 key 若未完成则 blocked |
| Tstage 登记后宕机 / key 创建后 seal 前死机 | 恢复补偿扫描受保护意图；可有无内容 metadata/孤儿 key，不能有明文对象 |
| 密文发布后 T1 rollback / attach 分类策略变更 | 版本事务回滚；密文不可读隔离并可恢复清理，不误删其他合法 Payload/key |
| 清理失败/删除前备份恢复 | 保持栅栏、blocked；最新撤销/删除登记优先，不自动恢复旧 key |
| 明文泄漏被检测 | 停内容路径、隔离、按受控副本事件处理，不以正常 TTL 清理冒充已消除泄漏 |

测试专用无加密合成 adapter 如将来需要，只能在隔离非生产 fixture 装配、禁真实数据/生产 credentials/备份、无公开监听并明确标记，不与生产发行装配共用。公开 API/schema 中没有启用字段；synthetic、NoModel、环境变量或客户端自述 test 均不能开启。当前新增 oracle 无加密 adapter，也没有实现该测试旁路。

## 兼容与验收

PayloadInput.classification 名称暂留，语义明确为声明；StagedPayload 新必填 effective_classification/protection_policy_epoch/encrypted，供未来 SDK 更新。旧 staging 没有可信保护记录一律隔离，重新加密/分类后才可绑定；不能补 encrypted=true 假迁移。现有 Event/Evidence/Revision 仍是无正文引用 DTO；真实分类关系在 ProtectionBinding/来源图中验证，Schema 不是密文检测器。

P0_SECURITY_ACCEPTANCE 列出模型正反例和 P1 实际磁盘/WAL/索引/日志/备份、rollback、key 故障与重试注入。契约测试通过不表示这些内容路径已实现或安全能力已通过。

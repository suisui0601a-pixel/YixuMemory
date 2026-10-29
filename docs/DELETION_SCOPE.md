# P0-SC：删除范围与来源副本边界

Status: Proposal / design contracts only，待 Luna 独立审查 R1。扩充 ADR-003，不修改已接受的事务、租户或部署架构。P1 首版目标是**同一 Space、明确登记关系图**，不是语义去重或租户全面擦除。此处“支持”是设计目标，不是已实现能力。

## 操作类别与完整性

| 操作 | 范围与根 | 必须处理 | 保留/拒绝 |
|---|---|---|---|
| memory_graph | space + Memory | Memory、全部历史 Revision 的独占内容、显式派生依赖、索引/缓存/导出/作业副本 | 保留独立来源 Event/Evidence 及其他合法 Memory 共享的来源；保留不是宣称来源已擦除 |
| evidence_graph | space + Evidence | 该 Evidence、所有关系类型的引用 Revision、派生副本；相关 Memory source_blocked | 若物理 Payload/key 共享，扩展到所有登记拥有者及其依赖，再对完整闭包授权；不只删一条 link |
| source_graph | space + Event（受控本地来源） | Event、其 Evidence、所有引用及派生 Revision、共享物理 Payload 的所有登记拥有者与依赖 | 同 Space 全部历史、supports/contradicts/context 都包含；来源外部原站及独立跨 Space 复制不宣称已删 |
| source_all_copies / subject_all_copies | tenant / 跨 Space | 将来需覆盖独立复制的来源/主体数据 | P1 首版明确 unsupported_erasure_scope，422，执行前拒绝，无局部 Tdel/任务、无 completed |

“全部副本”仍是完成的必要条件：**已接受操作的完整对象图及全部受控物理/派生副本**必须处理，包括远端受控存储、备份与 key 副本；不因存储在别处就排除。Space 限制是逻辑根/关系图范围，不是把已登记的 off-host 副本排除。不可把原先意图为来源/主体全面擦除的命令重命名为 source_graph 后悄悄执行。无法满足广义安全承诺时拒绝该类别；已经接纳但发现遗漏或无法证明完成时 blocked，继续拒读。

来源身份只指当前 Space 内受控 Event UUID，不表示 canonical source identity。source_kind、source_locator、相同文本/URL/hash 都不是跨 Space 完整枚举证明。两个 Space 各自导入相同内容是两个独立来源；擦除其中一个绝不返回租户级来源/主体完成状态。即便调用者有所有 Space 权限，也不支持自动跨 Space 语义发现。未来另行 ADR 设计 canonical source identity、replica registry、跨 Space 授权/协调与删除栅栏，本轮不实施。

## 对象图与事务

1. 在现有 tenant/Space 根锁、知识顺序及策略锁下校验目标和服务端完整图快照。图登记 Event→Evidence、全部 RevisionEvidence、Memory→Revision 所有权、Payload/locator/provenance、显式 DerivedLink，以及各受控存储/备份/任务 generation。所有派生写在现有 UoW 中登记依赖，未登记或不明归属拒绝写/删除预检。禁用跨 Space Payload/key 共享、跨 Space FK 和隐式全局内容去重。物理内容共享必须有完整反向拥有者集合。
2. memory_graph 默认不拥有独立 Event/Evidence，即使暂时没有其他引用，也不以 GC 代替来源擦除。其独占内容指 Revision/Memory 拥有的副本，不是独立来源本身。P1 新绑定要求每个 Revision 内容有独立可擦除 key 单元；若旧图存在 Memory 内容与外部合法拥有者共享且无法无损分离，**执行前拒绝 ambiguous_payload_ownership**，不得误擦除，也不得留下该 Memory 内容后谎报完成。
3. 来源/Evidence 删除按反向引用和显式派生边作固定点闭包。共享物理 Payload/key 会扩展到所有登记拥有者；闭包必须足够表达这种扩大，不能只更新首个 Memory。闭包包括历史版本及已发往受控索引/缓存的投影；不承诺自动发现第三方客户端自行复制的内容。第三方不可控副本明确报界外，广义请求不能以此完成。
4. **先对完整闭包授权，再原子 Tdel**。memory.erase 不自动等于 erase-source；共享来源需要现有额外来源擦除能力与覆盖所有受影响对象的服务端批准，不能由客户端自述授权。授权不足、图缺失、超预算、未知存储、范围不支持时，执行前返回拒绝/冲突，无部分擦除。预检至 Tdel 用同一锁/版本验证；图或 policy 漂移重算，不用旧预检执行。授权检查不得泄露未授权依赖对象清单。
5. Tdel 原子保存不可缩窄的闭包快照/graph_cut_off、generation、删除栅栏、RevisionInvalidation、outbox/audit。queued 已表示栅栏生效。新 link/派生/异步写必须重验栅栏并拒绝指向删除源，故闭包不会在冻结后漏入新引用。晚到旧 worker 仍按现有 fencing 规则拒绝/清理。发现 snapshot 未覆盖的受控副本只能扩大并授权协调或 blocked，不能缩小原意图。
6. Worker 按闭包 generation 擦除并收集每个实际存储/副本/key/备份回执；登记存储清单不能通过删条目缩小。未启用的存储类别也须以完整登记和空副本清单的受信验证回执证明不适用，不能省略类别或让请求者传 true；聚合回执必须覆盖其全部实际副本。请求人在 Tdel 后撤权也不撤销栅栏；已批准的有界擦除作业继续，新的扩大执行需重新批准。错误、清理失败、备份/key 副本不明或缺回执均 blocked，修复后幂等续行。

## R1 oracle 整图类型门槛（2026-10-29 修复提案）

前版只写了强类型引用，未逐项列出规范化 oracle 字段矩阵；仅检查 ID 存在导致 false-pass。本次用 contracts/security/graph-relations.json 明确当前投影，逐边验证后才计算闭包，不把旧版漏校验的接纳解释成类型支持承诺。

| 关系字段（键 → 值） | 合法端点 | 依据/边界 |
|---|---|---|
| evidence_events | Evidence → Event | 领域 Evidence.event_id 必须引用来源 Event |
| revision_evidence | Revision → 非空唯一 Evidence 列表 | 领域 RevisionEvidence，包含全部 relation，不以 supports 才算依赖 |
| revision_memory | Revision → Memory | 领域 Revision.memory_id，聚合归属不是内容派生 |
| derived_from | Event/Evidence/Revision → Event/Evidence/Revision 列表 | 内容派生，包括模型产物/捕获内容；保留原 Revision→Revision 以及三类内容实体的合法组合 |
| payload_owners | Payload opaque ID → 非空唯一 Event/Evidence/Revision/Memory owner 列表 | 保留原来源共享和 Memory 独占副本规则；不从 pa/source 等测试别名推测用途，不取消既有 memory_graph 共享歧义拒绝 |
| memory_versions | Memory → 正整数，排除 bool | 领域 Memory.version；对非当前目标的 Memory 也验证 |

derived_from 不代替 Evidence 的来源捕获或 Revision 的聚合 FK。Event/Evidence/Revision 都具有受控来源/内容，包含领域模型已定义的模型产物 Evidence，所以不能只允许 Revision 作为派生目标。Memory 的 current 指针会变，不是稳定内容来源：引用内容需使用具体 Revision；Memory 拥有的副本仍经 payload_owners/受控副本清单处理，而不是把可变聚合冒充内容版本。索引/缓存等受控副本仍在既有副本清单中。其他对象类别须先补显式矩阵、传播语义及正反例，不能静默猜测、降级或丢边。没有更改 R14 保护关系或公开接口。

为保持这些合法内容类型的完整闭包，Event→Evidence 捕获边对任一删除根遵循同一传播语义：Memory-only 擦除若经显式派生边到达新的 Event，也必须继续到该 Event 的 Evidence/Revision。未进入派生闭包的独立共享原始来源仍保留；不通过拒绝合法 Event/Evidence 派生边掩盖传播遗漏。

校验覆盖整个受检 Space 投影，包括当前删除遍历不会到达的组件：节点类型白名单；已知关系字段与端点；所有 Evidence 的 Event 行、所有 Revision 的证据/Memory 行、所有 Memory 的版本行完整对应；列表形状和重复；所有 owner/版本的类型。未知节点/关系字段、错误类型、悬空端点、缺必需行均拒绝，snapshot_complete=true 不能绕过。类型错误返回内部 invalid_graph，缺失关系返回 incomplete_graph；两者均只有拒绝结果和空 fence/erase 集合，原图不被修改，不返回部分闭包。外部错误仍遵循原 403/404 不泄漏策略，不输出未授权依赖详情。

新检查只证明合成规范化图符合该矩阵；不证明真实 SQLite 复合 FK、当前事务图快照完整性、实际删除协调器或逐副本 generation 回执正确，这些仍是 P1 运行验收。

## 读取、历史与替代证据

删除 source/Evidence 后所有受影响 Revision 具有不可逆 RevisionInvalidation（追加记录，不修改不可变事实正文）；相关 Memory 进入 source_blocked。Memory 自身删除仍走 deletion_pending→purged。source_blocked 的 current 不能返回有效断言，search/hydrate/history/cache/index/export/count/context 都以**当前**权威栅栏过滤；as_of、旧 commit token、缓存命中或索引水位不能豁免。可返回不含内容的授权删除状态，不输出失效 Revision 的正文、定位或摘要。受控客户端已发送内容的不可回收性不能冒称已解决。

独立替代 Evidence 到达不取消旧 RevisionInvalidation。只有相关来源删除均 completed 后，授权用户可显式追加 n+1 Revision，经 CAS、非空有效新证据、独立内容 Payload、新的完整派生关系及当前分类检查，将 source_blocked→active；parent 仍可指向旧版以保留结构。不能把旧 Payload/被删 Revision/受污染摘要作为“新证据”，不能直接修改 evidence_ids 或将旧版本恢复有效。新版本可读，旧版本内容继续 erased/不可读。多个并发删除须全部满足，不能只检查其中一项。

## DeletionRequest 与 API

原来的 space_id/target_type/target_id/status 不足以表达请求意图与完成覆盖。新增 p0-sc-1：operation、requested_scope、effective_scope、graph_cut_off、closure_ref/version、policy_epoch、generation、coverage、receipt_set_ref、requested_by。接受记录仅允许 requested_scope=effective_scope=space；completed 必须 coverage=space_graph_complete 且有效回执集合。Schema 只能验证形状，不把 receipt_ref/布尔值当完成证明；协调器须验证完整登记、同 generation 的所有回执和 in-flight writer 屏障。进度回执不返回敏感原文、稳定内容 hash 或原始 URI。

- 既有 DELETE Memory 固定 memory_graph/space。
- 既有 POST events/{event_id}/erasures 固定 source_graph/space，不再含糊称为所有同义来源擦除。
- 新草案 POST Space/deletions 接受显式 DeletionIntent（含 evidence_graph）。path/body 必须匹配。memory_graph 仍强制当前 Memory If-Match/CAS，缺失 428、过期 412；通用入口不能绕过原 DELETE 的并发前置条件。广义类别可被表达，但返回 422 unsupported_erasure_scope，不创建请求记录，不偷偷改写为 Space。权限失败按原 403/404 策略无存在性泄露。
- 对外没有成功但 effective_scope 小于 requested_scope 的响应。accepted 请求后来只完成一部分仍 blocked；不使用“partial completed”。

这是未发布 P0 草案的显式收紧：Deletion DTO 新必填字段、Memory 新 source_blocked 状态、source erasure 的局部含义及广义拒绝须让未来 SDK 同步。旧持久请求无范围记录时隔离为 unresolved/blocked，需重新确认原意图和授权，不默认补 space 后迁移为完成。已有广义 completed 声明也须重新审计，不能靠改 schema 追认。

验证见 P0_SECURITY_ACCEPTANCE：同 Space 两 Memory 共享 Evidence、独立跨 Space 复制、Memory 删除保留共享来源、部分失败保持栅栏、无权限无副作用、新证据不恢复旧 Revision。checks/security_closure.py 只是有界纯图/决策模型，不是实际删除协调器。

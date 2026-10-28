# PR #4 R1 图类型校验修复报告

Date: 2026-10-29 · Status: remediation proposal, awaiting Luna re-review; PR stays unmerged.
Reviewed predecessor: 4ecba8dd54b37e4e0794ce9badaa3115f9bd2ad4。

## 先复现，再修复

在 oracle 文件未改动、HEAD 仍为前序 SHA 时，先创建 test_evidence_event_wrong_kind_rejected_before_fencing 的两个参数用例，运行：

```sh
python -m pytest tests/test_pr4_r1_graph_types.py -q
```

旧代码结果为 **2 failed**，均在 `decision != admit_graph` 断言失败。实际观察如下（原图均未被修改）：

| 用例 | 唯一错误关系 | 旧 decision | 旧 invalid_revisions | 旧 fenced |
|---|---|---|---|---|
| 原始 fixture | evidence_events.e 从 event 改为 ma | admit_graph | ra, rb | event,e,ra,rb,ma,mb |
| 独立 Payload 变体 | 同一错误；来源 Payload owner 为 event，另一个 Evidence body owner 为 e | admit_graph | 空 | event |

第一例的物理共享 Payload 反向拥有者碰巧补全闭包，不能据此认定类型安全；第二例是合法独立存储布局，旧 oracle 漏掉两份依赖 Revision。修复后的两例均在遍历之前拒绝，空 fence/erase、原图不变。修复不是针对 ID 名称硬编码。

## 修复范围

新增整图 _graph_violation，接受闭包前验证：所有节点类型、全部关系字段及两端、列表形状/重复、悬空引用、必需 FK 行覆盖和每个 Memory 的正整数版本（排除 bool）。也检查与当前路径无关的组件，snapshot_complete=true 不能替代校验。未知关系/节点类型拒绝，不丢边、不修图、不返回部分结果。保留原 scope/授权/CAS/完整闭包及歧义所有权拒绝规则。

前序文件只写强类型引用，未展开当前 oracle 的字段级矩阵。本修复以 graph-relations.json 明确规范化投影：Evidence→Event，Revision→Evidence，Revision→Memory；derived_from 为 Event/Evidence/Revision 内容间派生，包含模型产物 Evidence；九种内容类型组合均有正例。Memory 可变 current 指针不当内容版本；来源捕获/聚合归属走各自的表，不混成派生边。为不限制合法 Event 派生，捕获边对 Memory 删除根也按同一语义传播；独立共享来源不会因此进入擦除集。未来其他 DerivedLink 端点不能从旧缺陷的接纳推定支持，须先定义类型与传播语义。合法 payload owner 保留 Event/Evidence/Revision/Memory，不依据 fixture ID 名推测用途；显式共享仍按既有闭包/Memory-only 歧义规则处理。详见 DELETION_SCOPE 和 DOMAIN_MODEL。

R14 的分类/加密架构、接口及已有函数不改；Event/Evidence/Revision/Memory DTO、staging、KeyVault 契约和原 525 项测试文件保持不变。矩阵约束针对内部合成投影，不宣称真实数据库/删除协调器已经安全。

## 测试清单

新增 tests/test_pr4_r1_graph_types.py **86 项**，原 **525** 项 ID 与测试文件完整保留，总计 **611**：

- 2 项 red→green 原始 false-pass 反例。
- 40 项：四类关系的全部错误已知端点类型组合，分别在 source_graph 和 memory_graph 下拒绝。
- 6 项各关系悬空引用；3 项版本键错误类型；7 项非目标 Memory 的非法版本值。
- 12 项未知关系/节点/owner 类型、重复/非法列表、必需行/表缺失。
- 1 项当前闭包外错误边仍整图拒绝。
- 9 项合法内容派生类型完整闭包；1 项 Memory-only 显式派生 Event 捕获链；4 项合法 Payload owner 类型；1 项机器矩阵与 oracle 无漂移。

所有负例断言拒绝且仅空 fence/erase，没有部分结果，原图逐项比较不变；正例核验完整 Revision/Memory/Payload 集合。原共享多 Memory、历史 Revision、派生闭包、Memory-only 来源保留、跨 Space 拒绝、源失效及 R14 全内容加密回归继续执行。

README 的 pytest、mypy、ruff、Schema/合并 OpenAPI、diff 与密钥扫描全部为提交门槛。另以全新归档复验、原 node ID 集合与受保护文件/函数比对确认范围；实际 CI、分支 SHA 与 PR HEAD 在提交后读取，不能从本文件推定未知远端结果已通过。

## 文件清单

- checks/security_closure.py
- tests/test_pr4_r1_graph_types.py
- contracts/security/graph-relations.json
- docs/DELETION_SCOPE.md
- docs/DOMAIN_MODEL.md
- docs/adr/010-deletion-scope-boundary.md
- docs/PR4_R1_REMEDIATION_REPORT.md

## P1 未执行事项

真实 SQLite 复合 FK、并发图快照/授权竞争、实际删除协调器、逐副本 generation 回执与恢复/晚 worker 故障注入仍待 P1。纯 checker、布尔观察和测试数量均不是数据库安全或真实删除能力证明。main、根 MIT LICENSE、原 P0 标签及授权 PR #2 不变。保持 PR #4 未合并，等待 Luna 对新 HEAD 独立复审，不自行宣布通过。

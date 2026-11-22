# R6-B3 Review Handoff

## 2026-11-22 F1/F2 修复增量（优先于下列历史记录）

失败基线 5fdfe3ef4c9515589531946deb876c78e33494d0；分支 local/p1-m3-w4a-r6-b3-remediation。

- F1：aabe4863026ac9428480bd4fed4a3220f4debde9，唯一权威闭包 + Closing 冻结记录；Fence/Applied/Reconcile 对齐；不完整依赖整单拒绝；隔离不能跳过 Grant。
- F2：2aa7ba6342884cdd06fb663d4c5e9933faba35ef，统一 MarkerState 判定；identity/observation 已提交后缺失即共享阻断；不重建 FileId；完整匹配回执保留合法对账。
- P11 新增 deletion_closure，仅空目录新建；旧 P11 契约被显式拒绝，无自动迁移。P9/P10、W1/W2/W3、锁文件不变。ADR-M3-016 必须独立复核。
- 新增5个测试，原47条保留。修复前 F1 47 pass/1 fail（依赖不完整却被接纳），F2 51 pass/1 fail（已观察 Marker 缺失未阻断）；最终52条定点通过。
- F1 的依赖缺边隔离场景分别以 A Grant 有效/单独到期执行，B Grant 保持有效；两种均须整单拒绝且无部分 Closing。另验证 Key 损坏隔离的授权拒绝。正常闭包成功测试验证冻结集合与执行集合相等。
- 证据目录 outputs/YixuMemory-R6-B3-Remediation-20261122，分别记录 F1/F2 原始失败及修复日志、完整回归、远端核验和源码归档。原 FAIL 审查及历史日志保持原样。
- 现有专用远端 https://github.com/suisui0601a-pixel/YixuMemory-P1-Remediation 为 Private；仅同步当前修复分支，无 PR、无其他分支/标签推送。
- 本轮是实施方自验；Luna 固定 HEAD 独立审查 NOT TESTED。B3/R6 及生产正文不自动获得通过；NoContentRuntime、RecoveryBlocked、Local/Protected 边界不变。

请先读 ADR-M3-016、闭包冻结/不完整拒绝路径、MarkerState 和新增五项测试，再结合原 FAIL 报告做增量独立复核。

## 固定对象

- 原始 B2：31549a5b6393c31f2d5bac40b3a9f27619eddb24；父 B1：fcbca0d4e097042734744f982c1c69d49b729f8b。
- ADR 阶段：03ebb6cede52cff998227d317fd76b193bb0731c。
- 实现固定提交：562b2d67912832acf6d19d1036469169b04a3363。
- 分支：local/p1-m3-w4a-r6-b3-deletion-recovery。
- 本 HANDOFF 为随后独立文档提交；最终 HEAD、源码 ZIP 及证据哈希见本地交付目录。
- 本地 outputs/YixuMemory-R6-B3-20261119；源码归档在 source，原始日志在 evidence，合成运行现场在 tmp。未推送或创建 PR。

## 首读

1. docs/M3_ADR_015_OBJECT_RECOVERY_DELETION.md。
2. docs/M3_R6_B3_DELETION_RECOVERY.md。
3. crates/sqlite/migrations/0011_object_recovery.sql；同时检查 profile10.rs 的生成 trigger、schema attest 和 P10 兼容路径。
4. profile10/b3.rs 与 recovery.rs、read.rs、write.rs。
5. profile10/b3/tests.rs、tests/test_profile11_contract.py、scripts/check_surface.py。
6. 外部 REPORT.md、INCREMENTAL_REVIEW.md、evidence/TEST_RESULTS.json、crash-artifact-metadata.json。

## 已运行

- B1/B2/B3 定点：47 harness 条目，保留原 24 条，新增 23 个业务断言测试；原集合含 owned_process_entry 辅助入口，不冒充 47 个独立生产安全能力。
- Workspace 默认：400 普通 harness 条目 + 10 compile-fail doctest；all-features：402 + 10，均通过。含辅助/子进程入口，两个配置不能相加成独立能力数量。
- 排除 16 个 Native 输出实验条目；命令在 meta 日志完整列出。本阶段没有 unfiltered Native 输出全量重验。
- fmt、Clippy all-targets/all-features -D warnings、默认/全特性构建通过。
- 54 个外部编译拒绝探针与 4 个依赖图检查通过。
- 公开契约基线 611、本地 Python 54（新增 P11 SQL 3）通过；P0 mypy/ruff 通过，本地 scripts/tests ruff 通过。
- NTFS / CurrentUser DPAPI / XChaCha20-Poly1305 为真实执行。仅合成正文；动态 UTF-8/UTF-16LE 哨兵产物扫描无匹配。
- 13 个真实子进程强杀点；保留现场的只读 SQLite 独立核对通过。进程终止不等于物理断电。
- Cargo.lock 无变化；59 个锁定 registry 包 OSV 审计 0 advisory；gitleaks 工作树/历史/暂存区无发现。不是未知漏洞或合法性担保。

## 重点反例

两项增量审查问题先复现为 44 pass / 2 fail，修复后通过，原始失败日志保留：

- 登记备份无解释消失不得获得 BackupExpired。
- 已有清除回执的原密文/包装再现必须阻断所有读取。

另检查 Source 的全部历史 Revision、独立 Grant 到期且无 Closing 的拒绝、相同字节不同 FileId 清理拒绝、对象损坏但其他 Memory 可读，以及多 Closing 不互相解除。

## 不能批准的能力

- 生产正文、HTTP/IPC/CLI 正文、生产身份、服务身份/ACL/DPAPI 跨身份迁移。
- Native Terminal Settlement、A3、R12、Protected Witness。
- 非正常 Owner 强杀后的成功接管；仍全局 RecoveryBlocked，只能诊断。
- 任意旧 P10 自动迁移；P11 仅空目录新建。
- 跨 Space 全面擦除、语义副本发现、外部/未登记备份销毁。
- 完整一致旧系统快照抗回滚、绝对密码学销毁、物理介质擦除。
- Linux/macOS/exFAT 支持保证或物理断电/磁盘故障验收。

## 独立审查状态

实施方结论：CONDITIONAL PASS，仅当前本地合成边界；R6 整体和生产内容 NOT READY。

有只读 Schema/增量审查，审查身份只能确认 Codex，无法核实底层型号。GPT-6 Luna High 固定 HEAD 签核仍 NOT TESTED。不得把此交付当作指定模型已签核。

请 Luna 对固定源码及原始反例增量复核，不通过报告自述替代代码。尤其验证局部 FK 诊断不会吞掉共享故障、marker/receipt 再现检查覆盖读写、erase 与 receipt 的未知窗口不自动签发成功，以及 stop hash 验证先于新隔离写入。

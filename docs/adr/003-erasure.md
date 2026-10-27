# ADR-003 不可变与真实擦除

Status: Accepted (design only) · Date: 2026-10-28

候选：永久原文事件（可追溯但无法删除）/直接删证据（破坏依赖）/不可变最小信封+可擦除 Payload（协调复杂）。选择第三项；删除共享来源需闭包失效、清副本、销毁所有可恢复 key，备份未处理不得 completed。兼容影响：历史读取可能返回 erased；事件重放不能恢复原文。验证：旧快照+旧 worker+旧 key+共享 evidence 反例，P1-04/05。

# ADR-005 版本、时间与数据契约

Status: Accepted (design only) · Date: 2026-10-28

候选：覆盖摘要（丢证据）/last-write-wins（隐藏冲突）/追加 revision+双时间+显式 Conflict。选第三项，事实/推断/虚构隔离；JSON Schema 形状校验与领域 FK/授权校验分离。兼容影响：REST/MCP/SDK 共享数据契约；unknown write fields 拒绝，主版本处理破坏性枚举变化。验证：schema 正反例、as_of 防未来泄漏、同父 CAS 和冲突保留，P0 合约与 P1-07。

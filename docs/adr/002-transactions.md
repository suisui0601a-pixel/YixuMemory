# ADR-002 事务与 outbox

Status: Accepted (design only) · Date: 2026-10-28

候选：DB+broker 双写（简单但丢消息）/DB outbox（增加轮询与去重）/分布式事务（复杂）。选择单 DB UoW+outbox、CAS、至少一次与 fencing；不承诺 exactly-once 外部副作用。兼容影响：所有 backend 保留同一幂等和 Conflict 语义。验证：commit/ACK 各断点 kill、租约过期竞争、同 key 异体 409，P1-02/03。

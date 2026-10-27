# ADR-004 授权与双后端

Status: Accepted (design only) · Date: 2026-10-28

候选：只靠客户端 tenant（不可接受）/仅数据库 RLS（SQLite 不支持）/服务端策略+复合 FK+PG RLS 防御。选第三项；tenant admin 不自动读内容。兼容影响：后端均需 AuthContext；SQLite 单写吞吐受限，不报未测试指标。验证：跨 tenant/space 引用、连接池 tenant 残留、撤权竞争，P0 SQLite 探针与 P1-06。

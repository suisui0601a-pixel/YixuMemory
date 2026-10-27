# 多租户权限与威胁模型

身份由服务端验证 issuer/audience/signature/expiry 后映射 Principal；请求中的 tenant/space 是选择目标，不赋予权限。个人 owner、role/team/project 成员来自服务端 Membership/Grant；JWT 中自述 admin/owner 不直接采信。默认拒绝，显式 capability；共享不跨 tenant，跨 tenant 导入视为新授权来源和新证据。

| 主体 | 本人 personal | role/team/project | 租户运维 | 删除 |
|---|---|---|---|---|
| 未认证 | 拒绝 | 拒绝 | 拒绝 | 拒绝 |
| owner | read/write/history/export | 按独立 Grant | 无默认权限 | 自己空间 memory；共享来源须额外权限 |
| reader | 仅显式 Grant read/history | read/history | 无 | 无 |
| contributor | 需 Grant | read/append/revise；不可管理成员 | 无 | 不自动允许 |
| space steward | 需 Grant | 管理空间 Grant、归档、批准来源删除 | 无 | erase（显式 capability） |
| tenant admin | 不自动读内容 | 不自动读内容 | tenant/membership/config/audit-metadata | 可停用，不自动 erase 内容 |
| privacy operator | 只获批准的 erase 范围 | 同左 | 最小审计+擦除进度 | 合规擦除，不自动读/export |
| worker service | 无用户全权 | 每 job 有界 capability + tenant/generation | 无全局查库接口 | 仅所分配 job |

服务端授权按 tenant status、principal status、membership 到期、capability、space、object lineage、删除栅栏检查；export/history/evidence/search/count/error/cache 全走相同策略。撤权递增 epoch，缓存键含 tenant/principal/epoch，缓存失效失败时拒绝读。数据库防线不能替代服务端检查；PG RLS 是防御层，连接设置用事务 SET LOCAL，池复用不能沿用旧 tenant；SQLite 无 RLS，仓储复合键强制隔离。

| 威胁/信任边界 | 控制 | 反例测试 |
|---|---|---|
| 客户端伪造 tenant/admin、IDOR | 认证映射+每对象授权+同租户 FK | A 用户猜 B 的 memory/evidence/job ID，全部拒绝且无存在性差别 |
| 恶意文档 prompt injection、记忆投毒 | Evidence 视为数据，模型无授权权；来源分类与人工确认 | 文档要求导出全部记忆；不能产生工具调用权限 |
| SQL/查询注入 | 参数化、白名单排序/过滤、限额 | 查询含 SQL 元字符仍只被当内容 |
| URL/附件 SSRF 与恶意文件 | 托管上传、禁止任意抓取；未来 fetch adapter 单独隔离 | 私网/重定向/超大文件均拒绝 |
| worker 重放、旧租约、删除后复活 | fencing+generation+最终水合+擦除屏障 | 延迟旧 job 在 erase 后 ACK/写入失败 |
| 索引/embedding/缓存泄漏 | 派生数据等同源敏感级别，分租户、限制外部供应商 | 撤权后旧 cache/vector hit 不可水合 |
| 并发撤权 TOCTOU | 权限根锁与 epoch 核验 | 撤权与 export 交错，无新授权窗口 |
| 日志、追踪、公开仓库泄密 | 不记录内容/Token，合成测试、安全扫描 | 植入假 secret 与 DB 路径扫描应失败 |
| key/备份复活 | 独立 key 生命周期、恢复隔离、删除登记 | 老备份恢复无法解密已删除内容 |
| 资源耗尽和侧信道 | 配额、超时、分页、统一错误 | 大过滤/无限 evidence 数被有界拒绝；统计无跨租户计数 |
| 内部运维滥用 | 最小权限、审批、审计、职责分离 | tenant admin 无 read grant 仍无法读 personal |

静态磁盘泄漏、云管理权限和供应商快照不在纯应用逻辑可完全证明范围；需部署级演练与审计。默认不向外部模型发送内容；启用必须明确 tenant policy、数据分类和供应商保留规则。审计不可包含原文、URI、稳定个人关联；公开访问日志不记录请求体。授权失败日志只记最小类别。

本基线无认证服务、RLS 或密码实现；上述为必须实现并测试的控制，不能描述为已获得安全保证。未来安全问题不公开附真实数据；通过 GitHub private vulnerability reporting（启用情况须核验）或由维护者指定安全渠道报告。

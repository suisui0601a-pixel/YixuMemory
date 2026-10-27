# 自托管部署契约 v1

Status: P0 design extension · ADR-008 · Contract version: `1.0`。以下 CLI、安装包和服务均为设计，尚未实现或发布。本轮不安装生产实例，不指定未经验证的安装命令。四个 Profile 是配置意图，不代表已认证兼容；postgres 是后端组合预设，可结合 desktop/container/server 的运行方式。

## 产品与架构边界

个人可在自己的电脑、NAS、VPS、独立服务器自托管。符合未来许可的个人非商业使用永久免费、不限个人设备数量，无注册、许可激活、定期联网验证或真实记忆上报。实例本地创建 Principal/凭据是数据访问安全，不是向官方注册产品账号。main 仍为 MIT；本设计不实施 PR #2 的许可提案。

部署控制面位于 L6；REST/MCP 在 L1，复用 L2 用例及 L4 Ports。部署助手不读取记忆，普通记忆客户端不管理安装。宿主机管理员或能读取进程/文件的主体实际上可绕过应用权限；要限制安装 AI，必须通过独立 OS 身份/受限 helper 实现，不能只换 token 名称。

## Profile 与兼容目标

| Profile | 目标平台与前置条件 | 数据/密钥 | 网络与备份 |
|---|---|---|---|
| desktop | Windows 11 x64、macOS 受厂商支持版本 arm64/x64、受支持 Linux x64/arm64；用户可写本地盘、文件锁/权限、可用端口；SQLite | 本地应用目录，OS keychain/凭据存储或独立受保护 keyfile | `127.0.0.1:8787`；IPv6 需显式启用 `::1`；SQLite 一致快照及独立 key/删除登记 |
| container | 单机 Linux 容器运行时、Compose v2 API 能力、持久卷及权限探针；Docker Desktop 是 Windows/macOS 的可选依赖 | 显式命名卷，容器内 `/var/lib/yixumemory`；只读挂载 secret/secret broker，不把 key 打入镜像 | host publish `127.0.0.1:8787`，容器私网可监听 `0.0.0.0:8787`；卷快照不能代替一致 DB 备份 |
| server | 受支持 Linux x64/arm64，独立服务用户，代理/TLS/认证配置；NAS 推荐此或 container | 默认 `/srv/yixumemory/<instance>`，owner/ACL 校验；独立密钥和备份位置 | 应用默认 loopback，代理对外 `443`；公网开放、防火墙、服务注册都需计划和授权 |
| postgres | 自托管 PostgreSQL 16–18 作为初始验证目标，驱动/迁移能力匹配；可与其他运行方式组合 | 数据库由 PG 管理，Payload/日志仍有实例目录；运行、迁移、备份角色与 DSN secret 分离 | 不 publish `5432` 到公网；私网/Unix socket；跨主机 TLS verify-full；数据库备份与 Payload/key 删除水位一致 |

平台/PG 版本是待实现验收矩阵，不是已支持的发布承诺；不设未测量的最低 RAM、吞吐或 RTO。doctor 报告 OS/架构、磁盘余量、runtime 版本、文件锁与 ACL 能力、时钟、端口占用、key backend 和备份可达性。安装依赖、下载、Docker socket、SSH、提权都不是 doctor 默认权限。首次 doctor 的 CLI/诊断启动包也必须来自已验证来源；下载另需网络预授权，禁止 curl-pipe-shell，离线包可校验后运行。无可信发行包时返回 prerequisite_missing，不能先执行未知安装脚本再补计划。Docker daemon/socket 访问可能具有宿主机高权限，不能因用户已有 Docker 就默认为安装 AI 授权；优先受限 helper，真实 rootless 支持待验收。SQLite 禁止直接把活跃 DB 放 SMB/NFS/网盘同步目录或让多机共用；NAS 进程应使用 NAS 本地受验证卷。见 [SQLite 部署选择](https://www.sqlite.org/whentouse.html)。

Profile 默认模板在 `contracts/deployment/profiles/`；`config.schema.json` 约束形状及安全基线，不能替代真实 ACL/防火墙检查。postgres Profile 采用 server 运行方式作示例，其他组合必须生成独立计划。

## 路径、持久化与秘密

Windows 根目录 `%LOCALAPPDATA%\YixuMemory\instances\<id>`；macOS `~/Library/Application Support/YixuMemory/instances/<id>`；Linux `${XDG_DATA_HOME:-~/.local/share}/yixumemory/instances/<id>`。server 根可按授权改变。`plan` 的绝对规范路径仅供展示和漂移诊断，不是安全写能力。`init` 等操作必须遵循 [FILESYSTEM_OPERATIONS](FILESYSTEM_OPERATIONS.md)：可信锚点、逐组件句柄相对/no-follow、身份/owner/ACL/卷核验、exclusive/no-replace；不允许检查后重新按字符串写入，能力不足 fail closed。程序锁只协调正常实例，不能据此证明防 TOCTOU。拒绝仓库、未知已有目录或未验证网络卷作为默认数据根。

实例结构：`instance.json`（实例 ID、schema、owned-path manifest，无秘密）、`config/`、`db/`、`payloads/`、`outbox/`（外部派生暂存，不另建写入真相）、`indexes/`、`diagnostics/`、`audit/`、`staging/`。DB 内 outbox 仍与写入同事务。备份位置须与活动数据分开，并检查故障域；仅另一子目录不算独立备份。

配置只存 `credential_ref`、`key_ref`，不用明文 DSN/password/token；秘密经 OS secret broker、继承句柄/受限 FD 或受 ACL 保护的文件传递。POSIX 目录目标 0700、secret 文件 0600；Windows 禁止 Everyone/其他非授权主体读取。容器 Linux 目标非 root UID、只读 rootfs、临时 tmpfs、无 Docker socket/host network；挂载卷 ACL 必须 doctor 验证。若 keychain 不可用，可由用户明确授权独立加密 keyfile+解锁机制；不能自动降为明文。无安全 key backend 时敏感写入 unavailable，符合原 P0 删除设计。

## CLI 命令与稳定输出

拟议统一命令 `yixumemory <command> --json`；命令实际二进制尚不存在。所有命令返回一个 UTF-8 JSON envelope，stdout 仅结果；诊断 stderr 同样脱敏。CLI envelope 不适用于 MCP stdio（后者 stdout 只能 JSON-RPC）。`result.schema.json`、`exit-codes.json` 是输出与退出码唯一机器契约；不以 stderr 是否为空判断成功。

| 命令 | 行为与副作用边界 | 重复执行 |
|---|---|---|
| doctor | 默认只读检查；可在明确允许的 scratch 目录做短暂探针，不能安装依赖、读取内容、导出秘密 | 相同环境生成相同 normalized facts digest；时间与请求 ID 不参与 digest |
| plan | 从配置+doctor 报告计算动作、路径、下载来源/digest、权限、端口、备份/迁移前置条件；不执行动作 | 同规范化输入（包括显式有效期）产生同 canonical plan hash；无任意 shell command 字段 |
| init | 验证计划和授权后初始化新实例、目录、配置及秘密引用；不启动网络服务 | owned manifest 与配置一致则 no-op；未知文件/不同实例 ID 拒绝；无 force 覆盖 |
| up | 启动已初始化实例；目标版本不同视为升级，需新计划、备份及迁移兼容门槛 | 已运行且版本/配置相同 no-op，不重建卷/重新生成 key |
| status | 脱敏健康与进度、版本/迁移/备份状态，不输出内容、Token 或完整 DSN | 只读；不自动修复、提权或升级 |
| connect | 检查已有的有界数据面 credential ref，生成客户端中立配置供审阅；默认 reader；不凭安装身份签发内容权限 | 先 preview；apply 仅句柄相对 no-replace 生成新管理文件；已有配置保持不变，不承诺通用 digest CAS 改写 |
| verify | 默认只读核验版本、连通性、认证拒绝、权限/安全 posture；写探针须对专用 synthetic Space 有显式授权 | 有界 ID/幂等键；不得将诊断成功视为全平台/删除能力已验收 |

`--non-interactive --plan <ref> --approval <ref>` 仅表示不弹窗，不授予权限；`--yes`（若未来支持）不能绕过审批/漂移/未知文件拒绝。安装依赖或系统服务不是隐式副作用，而是可枚举 action。停止服务、备份、隔离恢复验证、恢复、轮换使用受限运维 adapter 的类型化 action，经 plan/apply 执行；七个命令不承担无限制 shell 或未定义的 restore 子命令。

退出码：0 成功；2 输入/契约版本；3 环境前置不足；4 需要授权；5 权限拒绝/凭据错误；6 漂移、CAS 或未知文件冲突；7 安全策略阻断；8 瞬态不可用；9 部分完成需恢复；10 探针失败；11 备份/迁移/删除水位不兼容。同时发现多类失败时采用 exit-codes.json 的 error_precedence，部分完成标记优先于可重试错误，不能把已生效失败报告成可盲重试。

`SIGINT/SIGTERM` 转换为结构化取消结果仅在能安全处理时保证；强杀/断电可能无结果，下一次 status 从本地 journal 恢复，不能把无 JSON 当成功。

## 授权计划、状态与恢复

固定流程 doctor → plan → 核对授权 → init → up → connect → verify。无 doctor facts digest 不可 apply。plan 绑定实例、主机标识、规范 config digest、preflight digest、固定 artifact digest、schema/target version、目标路径/配置 preimage、动作、网络目标、有效期与执行身份。hash 用 RFC8785 等确定性 canonicalization（最终库待选），hash 覆盖有效期，排除 request_id 等随机信封字段；不能用含秘密内容计算；secret ref 被绑定但 secret 明文不参与输出。

approval 是受信任授权 broker/OS 边界产生的记录，绑定 plan hash、有效期、action ID 和每个 scope；文件形状不是授权证明。AI 自己生成 `approved:true` 或改 JSON 不会被认可。本轮仅接受 single-plan approval：action_ids 与规范化计划动作 ID 精确集合匹配、scopes 与计划所需 scope 精确集合匹配且无重复；allow_elevation 必须等于计划是否实际需要提权，并受额外受信 OS/helper 安全策略允许。计划需求由受信执行器结合实际操作推导，不能仅信 AI 声明无需提权。广义预授权未设计，不能通过多加动作/scope 或 allow_elevation=true 获得；后续如需必须独立对象与 ADR。动作或环境漂移返回 4/6 而不是沿用旧批准。系统权限、监听范围扩大、迁移/恢复、客户端配置改写、真实数据读写须各自相应授权。

动作状态 `planned → authorized → running → succeeded`；失败分为 `blocked`（未执行）、`failed`（未完成）、`recovery_required`（部分生效），仅允许基于 journal、相同 idempotency_key 和重新校验前置条件重入。journal 每动作写 before-intent 并 fsync，再执行，完成后写 receipt；崩溃后先探测真实状态，不能盲目重装。实例锁防两助手并发；外部副作用 generation/fencing 沿用 P0 原则。

生命周期 `absent → initialized → running → stopped`，升级中 `maintenance → running` 或 `recovery_required`。禁止 failed init 后清空数据目录；只能清理 manifest 证明本次新建、未承载原始数据的临时资源。重复 init 绝不删卷、复用 key 覆盖旧文件；adopt 已有实例必须有证明和新计划，不在初版自动化路径中。

## 备份、升级与恢复

SQLite 使用一致性 backup API/受控维护快照，不能仅复制运行中的单个 .db 忽略 WAL。PG 使用经验证的 dump/restore 或物理备份/PITR，备份方法及兼容范围记录到 manifest。见 [SQLite backup](https://www.sqlite.org/backup.html)、[PG backup](https://www.postgresql.org/docs/18/backup.html)。Payload、schema/checksum、knowledge_seq、auth_epoch、tombstone、删除登记水位和必要密钥引用必须成套记录；密钥备份仍受撤销/擦除要求，不为可恢复性保留已删除 key。

升级前 plan 明确写入围栏、兼容版本窗口、最近已验证备份及恢复演练回执、所需 schema 迁移。证据不足退出 11；仅 backup 文件存在不够。verify_backup 在计划绑定的独立临时目标验证隔离恢复，不能开放恢复实例网络；它用于取得 restore_drill 回执，不要求已有该回执。实际恢复与升级则必须通过已有的验证门槛。先停写/一致快照再 expand/backfill/verify/contract；未知较新 schema 拒绝启动。数据库 contract 后禁止简单回滚应用二进制；前向修复或恢复至新隔离目录，先重放最新删除登记和撤销 key、检查恢复水位，再切换。缺最新删除登记保持 blocked，绝不恢复已删除敏感内容上线。

诊断/journal 仅保存动作标识、版本、脱敏错误、目标 opaque refs 和校验结果；无内容、访问 token、密码、明文 DSN。备份和诊断不自动上传，支持用户审阅脱敏导出。实际保留期与本地磁盘配额需在实现时确定，不能形成无限增长。

## P0 检查与待运行验收

本轮：四 Profile schema/交叉约束、CLI envelope/退出码一致性、plan/approval 形状及引用、权限分离、工具映射、合成闭环 fixture 正反例。见 `SELF_HOSTED_ACCEPTANCE.md`。这些测试不启动实例或真实容器。

未来必须在每个目标 OS/架构与两后端真实执行冷启动/重复 init/中断恢复/密钥轮换/备份恢复/升级，验证 loopback、ACL、无模型断网 CRUD 和跨租户拒绝。资源建议、NAS 型号、签名发行渠道、key broker 库和性能数据均待实测；未完成前不能宣传为可生产安装或高并发已验证。

MCP 连接必须区分 modern/legacy wire 与 session，见 [MCP_COMPATIBILITY](MCP_COMPATIBILITY.md)。部署 oracle 的 broker_proof_verified 是合成可信观察占位，调用者提供 true 永不构成真实授权；生产签发者认证及受保护 IPC 尚未实现。

审批集合规范化忽略列表排列但不允许重复；实际动作执行顺序属于 plan hash，不能通过重排 approval 改变计划。proof_ref 必须由受信 broker 校验其绑定的完整审批记录、签发者、期限与权限；只是引用字符串或 true 标记不足以授权。

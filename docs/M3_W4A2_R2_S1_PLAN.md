# W4-A2-R2 S1 隔离实验手册

2026-11-10。S0 方向已批准。S1 当前只交付免提升预检和后续实验设计。正式决策见 [服务边界](M3_ADR_008_SERVICE_OUTPUT_OWNER.md)、[恢复准入](M3_ADR_009_OUTPUT_SETTLEMENT_AND_RECOVERY.md)、[身份迁移](M3_ADR_010_DPAPI_IDENTITY_REWRAP.md)。

## 可重放预检

在隔离仓库根目录，复用既有工具链和 target。先测 C:，低于 2.50 GiB 停止新增构建。PowerShell 命令中的工具路径与构建目录为本地约定。

```powershell
if ((Get-Volume -DriveLetter C).SizeRemaining -lt 2.5GB) { throw 'Low space' }
. ./scripts/enter-toolchain.ps1 -BuildRoot ../yixu-p1-build -ToolRoot D:/YixuMemory/work/p1-tooling
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_BUILD_JOBS = '1'
$manifest = 'experiments/w4a-r2-s1/Cargo.toml'
cargo fmt --check --manifest-path $manifest
cargo clippy --offline --locked --manifest-path $manifest -- -D warnings
cargo build --offline --locked --manifest-path $manifest
cargo run --offline --locked --manifest-path $manifest
```

每条命令须确认退出码为 0 后继续。独立 crate 只使用已缓存 windows-sys 和 windows-link。主 workspace、主锁文件与 runtime 无依赖路径指向它。探针拒绝提升权限、非交互服务会话和外部参数。只检查自己的 process/thread/token、私有事件和临时本机管道，SCM 只申请 CONNECT 和指定名称的 QUERY_STATUS。若同名服务存在或无法证明不存在，退出，禁止处理其进程。管道不传正文，不创建持久文件或 DB。DPAPI roundtrip 留给合成服务 Profile。当前预检只问路径长度，避免应用触发 Windows 初始化真实用户的 DPAPI master key。

## 证据矩阵

| 项目 | 本轮 | 下一次原生实验及通过条件 |
|---|---|---|
| S1-01 Windows / Token | PASS。Windows 10 Home 19045 x64。非提升、Primary、Session 1，TokenUser 有效，Profile 路径可解析。 | 在隔离服务进程重新记录实际 TokenUser、Service SID、组属性、完整性级别、特权与 Session。记录分类及匹配结果，避免记录用户路径。 |
| S1-02 虚拟服务身份 | 部分预检 PASS，OpenServiceW 返回 1060。服务身份 NOT RUN。 | 由 SCM 启动独立虚拟账户。核对服务配置、实际 TokenUser 和 Service SID，拒绝 LocalSystem、共享账户、非预期特权或身份错配。 |
| S1-03 DPAPI / Profile | 路径解析 PASS。DPAPI 与服务 Profile NOT RUN。 | 在新虚拟服务 Profile 中仅用公开合成 bytes 做 user scope UI_FORBIDDEN roundtrip。服务干净重启、无交互登录重启后同身份可用，普通用户和另一身份失败。记录缺失 Profile 的拒绝。不修改真实用户 Profile。 |
| S1-04 对象 DACL / 继承 | PASS，仅自有对象。进程、线程、Token ACE 数为 3/3/4，事件非继承。 | 检查服务创建期 process/thread/token DACL 与 owner。受控普通进程实际请求敏感权，测试 handle allowlist；验证无启动期暴露。 |
| S1-05 DUP_HANDLE | PASS，自进程基线。query-only 返回 5，具 DUP_HANDLE 返回 0。R1 跨进程历史证据另见原报告。 | 对自己的合成服务实例，从未提升普通客户端请求 PROCESS_DUP_HANDLE、VM/Token/线程权限，预期拒绝。仅对稳定确认属于该实验的对象操作。错误 PID 或句柄身份使实验无效。 |
| S1-06 SQLite / Vault ACL | NOT RUN。 | 仅创建 VM 实验根中的合成 DB/WAL/SHM/Vault 占位对象。从客户端尝试读、写、删、改 ACL 和父目录替换，均应拒绝。服务身份正向验证合法访问。 |
| S1-07 EXE / 配置替换 | NOT RUN。 | 测试普通客户端对实验 EXE、配置、信任根、DLL 搜索路径和父目录的有效权。若可替换则停止服务并阻断。 |
| S1-08 IPC 伪造 | 管道前置 PASS。重复首实例及空 DACL 连接均返回 5。双向认证 NOT RUN。 | 假服务抢占、同 SID 错应用主体、转发、错连接、重放、跨 realm、远程客户端、超额句柄。必须拒绝，日志只记录合成事件和状态。 |
| S1-09 启停与唯一实例 | NOT RUN。 | 在合成 realm 追踪 SCM 启停、旧进程 HANDLE、实例竞争、保留 reader 与未决写。新进程不能凭 SCM/锁状态取得 Ready。 |
| S1-10 无人登录与恢复 | NOT RUN。 | 专用 VM 的获批重启窗口内，先启服务并采集启动证据，再登录取证。记录 Profile 可用性及干净/崩溃分支。真实 Ready/内容路径另行验收。 |
| A3 / R12 成功恢复 | NOT RUN。 | 须完成 S1 后续关卡、Native 结算与独立审查。 |

## 需单独批准的管理员操作

优先选用快照可回滚、无生产资料的 Windows NTFS VM。项目所有者需先指定 VM、唯一 RunId 和重启窗口。准备阶段没有编写或运行注册脚本。下表是后续提升操作的批准边界。服务实验宿主及 IPC 验证器需在执行前按表单独审查，现有预检 EXE 不能当成服务宿主使用。

| 顺序 | 拟操作与准确对象 | 影响与恢复 |
|---|---|---|
| 0 | 创建 VM 快照，确认无同名服务、实验根、待回滚事务。解析绝对路径、卷与重解析点。 | 无可验证快照或同名冲突即停。需要确认 VM 含有且仅含允许的实验数据。 |
| 1 | 创建 `%ProgramData%\YixuMemory-S1-<RunId>\{bin,data,evidence}`。仅发布预先校验 SHA256 的合成服务宿主、固定参数与测试程序。 | 变更限唯一实验根。禁止跟随 junction，禁止触碰既有安装目录。证据先转移，清理仅限核验过的该根。 |
| 2 | SCM 注册 demand-start、SERVICE_WIN32_OWN_PROCESS 的 `YixuMemoryBroker-S1-<RunId>`，账户为对应 `NT SERVICE\YixuMemoryBroker-S1-<RunId>`。ImagePath 为带引号的受保护绝对 EXE 及固定实验参数。配置专用 SID、最小特权与服务对象 DACL。 | 仅该 SCM 对象，系统管理其服务注册。禁止直接修改 Registry。若虚拟账户创建失败或需要额外系统权限，停止，禁止改用 LocalSystem。先 Stop 并验证退出，随后 DeleteService。 |
| 3 | 仅在实验根内设置 ACL。bin/配置/信任根由管理员与 SYSTEM 管理，服务读取/执行，普通客户端不能写/删/改 owner。data 由服务写，普通客户端无直接访问。保护继承和父目录 delete-child 权。 | 不改 ProgramData 自身 ACL、系统 ACL 或用户 Profile。核验可启动与有效权限。失败后使用 VM 快照撤销，禁止以放宽权限补救。 |
| 4 | 启动唯一服务。验证 TokenUser / Service SID。允许该合成服务 Profile 在专用 VM 内由 Windows 按需初始化，只向其封装公开合成 bytes。用既有普通测试账号运行攻击探针。 | 不创建系统账号或修改密码。仅本机合成 pipe，无网络服务。只访问自身服务进程和实验对象。OS 管理的 Profile 状态随 VM 快照回滚，禁止手工删除或改写系统 Profile。 |
| 5 | 另获准重启后，仅将该实验服务设为自动启动并重启专用 VM 一次。无人登录阶段由服务写最小合成证据。 | 影响整台实验 VM。结束时恢复 demand-start，停止服务，验证无活动操作与进程，导出证据后删除唯一实验服务或回滚 VM。 |
| 6 | 在导出所需证据后，回滚 VM 到预检快照。 | 对账服务名、根目录、Profile 及残留进程。若没有能力确认清理，记录 PARTIAL，禁止扩大删除范围。 |

任何额外操作，例如新账户、登录权调整、机器级 ACL、配置服务账号密码或安装驱动，都需要重新定界和批准。带后缀服务只验证相同部署模型。正式服务名及账户需要另外核验。

## 兼容性及本地交付

| 基线 | 核对范围 |
|---|---|
| W1 | NTFS 受控路径与句柄层仍由未来服务复用。预检独立创建 pipe，不触碰 PayloadStore。新服务目录权限需要下一关验证。 |
| W2 | 当前用户 DPAPI、KeyId、Generation 与 tombstone 保持既有逻辑。虚拟服务账户 Profile 与重封装都待独立验收。 |
| W3 | 统一排序器和 ReleaseIntent 语义保留。W3 合成历史启动路径不被服务宿主复用。 |
| W4 Profile 9 | schema、checksum、旧实例、DB 恢复及 NoContentRuntime 均无源码变化。需要新证据结构时走独立 Profile 变更。 |

R1 固定审查和原生结果来自原报告及 Luna 记录。R2 设计来自 2026-11-08 的 ARCHITECTURE_REVIEW.md、REFERENCES.md，并继承 2026-11-05 W4_ARCHITECTURE_REPORT.md、W4_CONTRACTS.md。当前不是重跑 R1。完整 runtime 回归未重建，兼容性依据固定基线与差异范围。报告不含真实身份值、Profile 路径、密钥和正文。

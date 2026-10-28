# 可信目录句柄内的文件操作契约（PR #3 A2）

Status: design only / OS race acceptance not executed。适用于 init/up/connect、诊断/journal、临时文件、备份恢复与清理。plan 的绝对路径只供展示；realpath+重查+程序锁不构成对恶意重命名或重解析竞态的充分防线。实际执行必须经受限 native helper，不能把检查成功当作允许任意路径字符串写入。

## 假设与可信锚点

可信根由受信 broker/helper 从受保护 OS 命名空间取得并持续持有目录句柄，验证 owner/SID、DACL/POSIX ACL、预期目录类型、卷与文件身份。锚点自身及祖先不能允许威胁模型内非授权主体删除、重命名、替换或改变 ACL；无法确认即退出 7 security_blocked。用户提供的 anchor_ref、owner_ref、ID 或 approved:true 都不是实际句柄/证明；plan 的 path_bindings 显式把 anchor_ref、anchor/volume identity、owner、ACL digest、预期类型与相对组件纳入 hash；执行时与 live handles 比对。序列化记录只作期望值，不能从路径重新“恢复”可信句柄。

保护针对不能修改可信祖先/进程/安全主体的其他用户、非特权进程和不受信请求。root/Administrator、内核/恶意文件系统、能更改 mount namespace 或已完全控制同一 OS 安全主体/进程、注入或复制 helper handle 的攻击者超出此保护保证。程序 mutex 只协调正常实例，并不阻止上述权限攻击。若普通 AI 具有同用户任意 shell，它不在受限 AI 的隔离假设内；需独立身份、sandbox/受限 helper，不能声称 token 分权即可抗宿主机控制。

## 必须满足的操作不变量

1. 从已验证锚点句柄出发，仅按单组件相对名逐层打开；保持链上目录句柄直到 receipt/fsync 完成。每个组件拒绝 symlink、junction、mount/reparse 等跳转，核验 handle 的对象类型、owner/ACL、文件/目录 identity 与预期 volume。初版只管理受控 ASCII 名称，拒绝点路径、分隔符、NUL、Windows ADS/设备名/尾点空格；任意已有目录 adopt 不属于自动流程。
2. 检查与使用同一打开对象；不得检查 handle 后 close，再以 absolute_path、GetFinalPathName、realpath 或拼接字符串 fopen/open 写回。权限与祖先约束须覆盖整个操作窗口。计划后 identity/ACL/volume 不匹配返回 6/7，重新计划，不自动接受新目录。
3. 新文件先在同一受保护目录中以 exclusive/no-follow 创建 staging，写入并 flush/fsync；用句柄相对、同卷、atomic no-replace 发布至新名字，目录也做持久化屏障。不允许 O_TRUNC、CREATE_ALWAYS、ReplaceIfExists=true、普通 rename 覆盖语义。EEXIST/名称冲突必须保留原目标字节；unknown 对象不打开为可写。
4. 已存在且确认为本实例的相同配置可从安全 handle 只读比较后 no-op；不同内容拒绝。初版不承诺通用 inode-CAS 替换，因为 POSIX rename 并不提供“仅当目标 inode 仍等于 X”条件。connect apply 只生成新的独立管理文件；已有客户端配置只做 preview，合并编辑须另行实现经过安全审查的专用 adapter，不以 digest 检查后普通 rename 覆盖。升级生成新配置 generation，保留旧文件，不原位改写。这里收紧原 PR #3 的泛化配置 CAS 描述。
5. 原子 mkdir 通常不返回目录 FD：创建后打开这一步只有在已证明父目录不可被威胁主体更改时才可接受；否则必须拒绝，不能用“mkdir 后很快 open”作为安全理由。创建、重命名、清理涉及所有父目录均满足此要求。
6. 文件身份至少 Linux/macOS 的 device+inode（平台可补充 generation/mount ID），Windows 的 VolumeSerialNumber+FILE_ID_128；数字 ID 可能重用，不能只根据旧 journal 数字重新获取权限，必须重新从受保护锚点验证并保留 live handles。普通文件拒绝不期望的 hardlink/link count，防可写内容逃逸。
7. 跨卷、不可核实 ACL/owner、身份不稳定、原语不支持/返回不明确、可被非授权主体改写的祖先、网络/未验证 FUSE/SMB/NFS/FAT 等环境全部 fail closed。不能回退为字符串 API、copy-delete 或先删目标再 rename。

## 平台实现候选与限制

| 平台 | 可采用的原语组合 | 必须探测及限制 |
|---|---|---|
| Linux | 锚点 FD；openat2(dirfd, relative) 加 RESOLVE_BENEATH、RESOLVE_NO_SYMLINKS、RESOLVE_NO_MAGICLINKS、RESOLVE_NO_XDEV；目录 O_DIRECTORY，文件 O_CREAT\|O_EXCL\|O_NOFOLLOW；fstat/fstatfs/ACL；mkdirat；renameat2(..., RENAME_NOREPLACE)；unlinkat 相对受保护父 FD | openat2/renameat2 与文件系统 flag 能力必须实际可用；遇 ENOSYS/EINVAL/EOPNOTSUPP、EXDEV 或无法保护祖先即拒绝；不透明降为普通 open/rename |
| macOS | 可信 directory FD；逐单组件 openat(..., O_NOFOLLOW\|O_DIRECTORY)，fstat 与 ACL/volume 核验；mkdirat；同目录 staging；renameatx_np(..., RENAME_EXCL)；unlinkat | O_NOFOLLOW 只约束单次 leaf，不能拿整段路径跳过祖先。APFS/其他 FS 的 EXCL 与持久化能力须探测；没有 openat2 等价全路径约束，依赖持有 handles+受保护祖先，不满足就拒绝 |
| Windows | 可信 directory HANDLE；NtCreateFile 的 OBJECT_ATTRIBUTES.RootDirectory+单组件 ObjectName，OBJ_DONT_REPARSE；FILE_OPEN_REPARSE_POINT 后查询并拒绝任意 reparse tag；新建用 FILE_CREATE，目录用 FILE_DIRECTORY_FILE；GetFileInformationByHandleEx(FileIdInfo/FileAttributeTagInfo)+owner/DACL；SetFileInformationByHandle rename 到目标 RootDirectory，ReplaceIfExists=false | 优先验证本地 NTFS；使用适当共享模式阻止 delete/rename，不能假设所有文件系统/驱动支持。CreateFileW 的 OPEN_REPARSE_POINT 单独不足以保护中间组件。不能保留句柄/身份、检查失败或 native wrapper 不可用就拒绝；不能回退 Win32 全路径创建 |

原语依据：[Linux openat2](https://man7.org/linux/man-pages/man2/openat2.2.html)、[Linux rename](https://man7.org/linux/man-pages/man2/rename.2.html)、[Apple open](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/open.2.html)、[Apple rename 源文档](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/rename.2)、[NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile)、[OBJECT_ATTRIBUTES](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes)、[FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)、[FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)。上述是待实现的可行组合，不是已测试 wrapper；SDK/系统版本、权限及 FS 不满足任一项就不能安装到该目标。

## 清理、失败与恢复

清理不是 rm -rf 或字符串递归枚举。仅能针对本次 journal receipt 证明新建且当前 handle identity 仍匹配的对象，从同一受保护父句柄操作；遇到未知后代、替换、reparse、类型或卷不匹配就停止并保留现场。Windows 优先按已打开对象 handle 设置删除 disposition；POSIX unlinkat 没有通用“删除该 inode”的 CAS，所以要求父目录在整个窗口内不可被非授权改写，且 entry 与 retained handle 匹配，否则不删。不能把 check-then-unlink 说成无条件安全。

恢复到新的 exclusive 目录，不能覆盖旧数据。解包也走同一 helper，拒绝 archive absolute/../symlink/hardlink 和跨卷条目；不得把第三方工具的任意 --output-directory 视为符合此契约。helper 无法约束的子工具先在独立 sandbox 验证，再仅以受控新对象发布。恢复仍须应用 P0 最新删除登记与 revoked key，文件安全不替代数据一致性或删除屏障。

## 具体竞争验收设计

真实运行测试采用双进程、barrier/fault hook，攻击进程只修改合成测试树。记录 syscall trace、FD/handle identity、目标 bytes digest、外部 sentinel 字节与 journal；每个阶段在 barrier 处插入替换，多次重复并用不支持原语的注入验证拒绝。不能为“测试方便”用管理员越过生产安全假设后声称所有权限攻击均可防御。

| 场景 | 注入点/动作 | 必须断言 |
|---|---|---|
| plan 后父目录替换 | plan 后、helper acquire 前 rename 父目录并放 symlink/junction | identity/可信祖先检查拒绝；外部 sentinel 不变 |
| 创建过程中组件替换 | mkdir 与打开新组件 barrier；攻击者 rename/替换 | 无权限更改受保护父目录，或 helper 检测并拒绝；不得向替换目标写入 |
| 发布配置前替换 | staging fsync 后将目标名放为 symlink/junction/普通文件 | no-replace 失败，原目标字节不变，无 follow/truncate |
| rename/reparse/跨卷 | 尝试移出锚点、mount/junction 到另一卷、EXDEV | 不继续 publish、不 copy-delete fallback，不越锚点 |
| 已有目标 | 目标预放唯一字节串，覆盖尝试与并发 EEXIST | 字节和身份不变，状态 conflict；只允许已验证相同对象只读 no-op |
| 失败后清理替换 | 写失败后将 owned leaf/后代替换为外部引用或其他目录 | cleanup 拒绝，保留未知对象/外部 sentinel，不递归误删 |
| 能力/ACL 不足 | 模拟无 no-replace、无 ACL、未知 FS、可写祖先、审计失败 | 退出 security_blocked，未进行任何写/清理 fallback |

本轮 filesystem-operation.schema.json、filesystem-policy.json、filesystem_violations 与合成 observations 只验证规范形状和拒绝逻辑；未调用上述 OS 原语，**实际 OS 竞争测试尚未运行**。plan 重查和 lock 仍用于正常冲突管理，但不再作为竞态防护证明。

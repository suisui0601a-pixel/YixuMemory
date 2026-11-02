# 项目测试状态总记录

更新：2026-11-03。当前平台决策以[平台验证矩阵](PLATFORM_SUPPORT.md)为准。历史报告中的统计保留其原始时间及范围，不能合并成生产安全认证。

## Windows NTFS 本地 P1 实验

本地 M3-B 固定审查提交标识为 `9b7c46175b826b1714835a6ec20b4bf351643a08`。这是未公开本地历史的标识，不能当成可在本公开仓库下载的提交或发行版本。

对应本地报告 `YixuMemory-P1-M3B-Persistence-20261102/REPORT.md` 记录 Windows C: NTFS、SQLite 3.54.1、Rust 1.99.1 GNU；报告及证据保存在项目所有者的本地工作区，不向本 PR 附带源码或原始服务器日志。

该合成持久化原型通过限定范围的独立源码增量审查。实现方记录的回归为 Rust all-features 265 个运行测试函数加 10 个文档测试、默认 263 加 10，包含新增 24 个函数，其中 22 个执行断言、2 个是子进程辅助入口；Python 公开基线 611 项及本地 Schema/OpenAPI 40 项通过。审查者没有独立重跑整个套件，不能把这些计数视为独立的安全保证数量。

真实 SQLite、文件重启和进程终止实验均限于受控合成装配。测试 key 可公开推导；没有真实保密、密钥撤销或 Witness 能力。Windows 命名原子发布、目录持久化、敌对 reparse-point、实际磁盘故障和物理断电仍未验收。正式 NoContentRuntime 继续拒绝内容操作。

## M3-C1 Linux：NOT TESTED / SKIPPED

- 当前状态：**NOT TESTED / SKIPPED**，不是 Linux 实现测试失败。
- 原因：服务器缺少系统 C 构建依赖，项目所有者决定当前阶段不继续投入 Linux 实机测试，不再为此安装依赖或部署外部测试环境。
- 环境只读预检：Ubuntu 22.04.5、Kernel 5.15、ext4。最初未找到 Rust/Cargo；后续虽获一次性隔离工具链安装授权，仍在安装前因缺少 C 编译器、开发头文件和链接启动文件停止。
- 实际活动：没有安装工具链、没有构建或执行 M3-C1 测试、没有创建实验目录或留下服务器实验资源。服务器 Docker/1Panel、容器、Volume、网络、镜像集合与监听端口经前后只读复核未发现状态变化。
- 历史证据：两轮原始 `PRECHECK_BLOCKED` 环境记录保留。历史报告的 FAIL 指当时前置条件受阻，不是编译、运行或安全用例失败；当前项目测试状态明确替换为 NOT TESTED / SKIPPED，不改写原始 stdout。
- 未执行：openat2、目录 FD、symlink/路径穿越、TOCTOU、no-replace、fsync、并发发布、文件替换/缺失/损坏、进程终止和恢复。模拟故障及物理断电也未执行。

本地完整记录目录：`YixuMemory-M3C1-Linux-Preflight-20261103` 与 `YixuMemory-M3C1-Isolated-Toolchain-Preflight-20261103`。当前报告、原始报告快照、预检 stdout 和校验清单均在本地保留；本公开摘要不披露服务器地址、凭据位置、业务路径或资源明细。

## 其他平台与 CI

Windows exFAT 和 macOS 未测试；没有 macOS Intel/Apple Silicon 兼容性证据。Linux/macOS 当前没有 P1 原型编译、运行及安全验收证据。公开 P0 的 Ubuntu Python CI 与本地 P1/M3 运行证据是不同范围，不能互相替代。

未测试不等于永久不支持。后续若重启跨平台验证，应重新确定源码基线、平台/文件系统、授权、资源预算和验收范围，记录实际运行结果后再更新矩阵。

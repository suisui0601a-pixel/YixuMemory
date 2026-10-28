# 自托管与 AI 接入验收边界

Status: P0 design checks only。当前没有可运行 CLI、REST/MCP 服务、容器镜像或核心引擎；合成 fixture/策略 oracle 不是对运行安全的证明。原 P0_ACCEPTANCE 继续适用。本轮所有检查进入现有 baseline CI，不跳过原 41 项检查。

## 本轮可执行检查

```sh
python -m pytest -q
python -m mypy checks tests
python -m ruff check .
python checks/security_scan.py
git diff --check
```

密钥扫描还包括 staged Gitleaks 与 CI 中全 Git 历史 Gitleaks。测试不需模型、Docker、PostgreSQL、网络或真实秘密；使用固定 UTC 时间与合成 UUID。checks/deployment_contracts.py 是无副作用的契约审查 oracle，输入中的 OS facts/broker_proof_verified 仅模拟未来受信检查结果；不执行签名校验、文件 ACL、网络探测或授权签发，不能拿传入 true 的成功结果当真实安装授权。

| ID | 当前检查 | 断言与可否证明的范围 |
|---|---|---|
| SD-01 | schema 自校验与四 Profile | 形状有效，无激活/设备数限制/外部模型依赖；非实机兼容认证 |
| SD-02 | unsafe_profile_mutations | native 通配、host 公网、DB publication、匿名、明文代理、未知覆盖、网络盘、缺卷/key/PG TLS 等拒绝 |
| SD-03 | plan_semantic_reference | host/artifact/config/facts/hash 漂移、无批准、过期、假 proof、缺动作/scope、提权、缺备份/删除水位、无审计均拒绝 |
| SD-04 | CLI envelopes/exit table | 七命令、0/2..11 与稳定错误/重试分类一致，成功/失败不能混淆 |
| SD-05 | client-config 与 no-secret 反例 | audience、scope、单 Space、TLS/URL、未知秘密字段及任意 shell 字段拒绝 |
| SD-06 | merged OpenAPI 与 catalog | 安全方案不变、原域 DTO 不变、13 工具 operationId 有效；现有 search 仅可选水位 header 扩展 |
| SD-07 | synthetic_loop | 工具输入/结果 schema、来源图、版本指针、词法查询结果、上下文 source/trust 与读己之写 token 一致；不是服务执行 |
| SD-08 | memory_authorization_reference | synthetic 服务端绑定下跨 tenant/Space、安装 audience、未授删除或上下文证据能力拒绝；不是完整真实鉴权 |
| SD-09 | scope/Git 检查 | main/旧 tag/LICENSE/PR #2 不变，本分支来自 main，独立 PR 与 CI、远端 head 同步 |

## 未来运行验收（本轮均未执行）

| ID | 实际执行步骤 | 通过条件 |
|---|---|---|
| RUN-01 | 每个目标 OS/架构、NAS 本地卷与 Docker 冷启动→重复 init→重启 | 保留字节/实例 ID/key/卷，unknown 文件/junction 竞争拒绝；无 root 默认要求 |
| RUN-02 | 无官方账号/无激活/断网且模型网络阻断，多实例同个人设备运行 | NoModel 确定性写入、词法检索、修订、管理可用；无遥测上传和设备数 gate |
| RUN-03 | 受限安装 AI 与普通 AI 分身份，越 audience、OS 文件与 helper 操作 | 数据/部署权限双向隔离，普通 AI 无 DB/key/Docker socket，禁止伪批准 |
| RUN-04 | socket/IPv4/IPv6/Host/Origin/代理 header/防火墙/DB 探测 | 默认 host loopback，远端 401/403，TLS 验证，PG 不公网，直连不能绕过 |
| RUN-05 | REST/stdio/HTTP MCP 实际客户端，协商、工具分页/撤权、SSE 中断重连 | stdout 协议纯净，OAuth audience/PKCE/redirect 正确，幂等与 CAS 不被运输层破坏 |
| RUN-06 | 专用合成 Space 按 synthetic-loop 工具链执行、重复同幂等键、跨 B 查询 | 单一版本/来源链，search/context 水位达标，跨域拒绝，无管理或删除默认权限 |
| RUN-07 | 安装每个 journal 断点断电/强杀，包损坏、磁盘满、锁争用、审计失败 | 不覆盖数据，保留诊断，无盲重装；外部效果先探测再恢复 |
| RUN-08 | SQLite WAL/PG+Payload 一致备份，升级中断、未知 schema、隔离恢复 | schema/数据/知识序一致；验证回执不足拒绝升级，不简单回滚已 contract DB |
| RUN-09 | 删除前备份/旧 key/旧 worker 恢复后尝试读 | 最新删除水位/撤销重放，无法复活；无法证明清理时保持 blocked |
| RUN-10 | client、payload、TLS/OAuth signing key 轮换与泄露紧急撤销 | 旧权限失效、必要恢复仍可用、已删除 key 不复活、所有日志无秘密 |
| RUN-11 | 上下文水合期间撤权/删除/换 revision，超预算、虚构域、提示注入 | 输出对应授权快照版本，变化时中止，不能把记忆文字提升系统权限；外发需批准 |
| RUN-12 | 客户端原配置含其他 MCP 项，connect preview/apply 并发编辑 | preview 保留原配置，apply 仅 no-replace 新管理文件，无 Token；直接改写既有配置不在初版范围 |

兼容目标、资源、RPO/RTO、NAS 型号、OAuth issuer、key backend、签名包/发布渠道未实测前均不得标为生产验收通过。P0 设计通过只说明可评审文档与契约检查通过。

## PR #3 FAIL 定点修复的追加验收

- A1：mcp-wire-examples、双时代 schema/矩阵、逐请求 metadata/header 语义、legacy 初始化状态、modern discover 可选客户端调用、版本错误和禁止跨时代降级。见 MCP_COMPATIBILITY 的真实 SDK 门槛。
- A2：filesystem-operation/policy 与纯观察 oracle 覆盖 plan 后父目录替换、创建组件、发布前 symlink/junction、rename/跨卷/未知已有文件、失败后清理身份替换和原语不足。**没有实际调用 OS 竞态原语，真实多进程竞争套件尚未执行**，详见 FILESYSTEM_OPERATIONS。
- A3：精确 action/scope 集合、重复/额外/admin scope、额外动作、提权相等及受信安全策略；原 137 个 node ID 保留并继续执行。真实 broker/IPC/签名验证仍未实现，不能由此把 caller bool 当真实审批。

提交检查还须比对被审查 SHA 的 pytest collection，报告新增 node ID 数与原用例保留情况；不得删除/skip 原用例掩盖审查发现。最终只报告 remediation ready，等待 Luna 对新 HEAD 再审。

## PR #3 第三轮 A1 追加检查

固定前序 15b12a0 的 245 个测试 ID 全部保留；A1 原宽松响应断言改为官方 Schema+产品 profile，原 12 个降级向量保留，仅纠正 5 个不安全的历史预期。A2/A3 测试函数及实现不改动。新增测试验证必填字段删除的实际 ValidationError、修复后的通过、类型/枚举/负 TTL/跨时代、官方与产品限制差异、原 -32603 反例及结构化错误矩阵。当前只运行离线契约检查。

RUN-05 追加：真实 SDK 在两时代/两传输下验证 Schema、SSE 最终帧、阶段状态机、缓存的当前认证绑定；discover/tools/list 的 HTTP header、代理缓存禁用、撤权/轮换/切换主体后不得复用过期能力；故障不得自动触发旧握手或写请求重放。仍须执行全部既定 OS 竞态/Windows 目录持久化、真实审批 broker 等运行门槛，不把既往 A2/A3 契约通过当成运行通过。

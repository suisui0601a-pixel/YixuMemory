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
| RUN-12 | 客户端原配置含其他 MCP 项，connect preview/apply 并发编辑 | 精确 diff/CAS，保留其他配置、无 Token；不支持 secret 注入时拒绝 |

兼容目标、资源、RPO/RTO、NAS 型号、OAuth issuer、key backend、签名包/发布渠道未实测前均不得标为生产验收通过。P0 设计通过只说明可评审文档与契约检查通过。

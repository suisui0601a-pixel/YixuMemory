# 忆序 · YixuMemory

Independent Long-Term Memory Infrastructure

Version: `0.0.0-p0` · Architecture: `P0-Baseline-001`

独立开源长期记忆内核的首轮架构基线。当前交付为设计、数据契约与验收框架，**没有可运行的记忆服务**。不承诺消除 AI 遗忘，也不宣称未经测试的吞吐、安全或后端等价性。

## 当前平台验证范围

当前可验证的原型测试范围为 **Windows + NTFS**。本地 M3-B 合成持久化原型已在限定范围内通过独立审查；这不是生产级安全认证，也不表示本仓库已提供该实现或可安装产品。不将结果推广到其他 Windows 文件系统，Windows + exFAT 未测试、不作兼容性或安全性保证。

Linux 和 macOS 尚未经过本地 P1 原型的编译、运行及安全验收，不作兼容性承诺。M3-C1 Linux 实机测试未执行，现按项目决定标记 **NOT TESTED / SKIPPED**。未测试不等于永久不支持，未来可以重新安排跨平台验证。公开 P0 的 Ubuntu CI 契约检查不构成 P1 Linux 原型验收。

详见[平台验证矩阵](docs/PLATFORM_SUPPORT.md)与[测试状态总记录](docs/TEST_STATUS.md)。

## 阅读顺序

1. [架构](docs/ARCHITECTURE.md) 与 [领域模型](docs/DOMAIN_MODEL.md)
2. [生命周期](docs/MEMORY_LIFECYCLE.md)、[时序](docs/SEQUENCES.md)、[一致性](docs/CONSISTENCY.md)
3. [安全](docs/SECURITY_MODEL.md)、[兼容](docs/COMPATIBILITY.md)、[ADR](docs/adr/README.md)
4. [验收](docs/P0_ACCEPTANCE.md)、[独立审查风险](docs/RISK_REGISTER.md)
5. [同步制度](docs/SYNC_POLICY.md)、[Agent 规则](AGENTS.md)

## 本地验证（无需模型、数据库服务或真实记忆）

Python 3.12+：

```sh
python -m venv work/venv
# Windows: work/venv/Scripts/python；POSIX: work/venv/bin/python
python -m pip install -r requirements-dev.lock.txt
python -m pytest -q
python -m mypy checks tests
python -m ruff check .
python checks/security_scan.py
```

请在已激活的虚拟环境执行 pip 与检查。`tests/` 验证契约、反例及 SQLite 隔离探针，不是生产引擎的集成测试；后端、删除及授权验收仍有明确待实现项目。

所有数据示例均为合成数据；不上传真实聊天、密钥、数据库或环境文件。基线标签采用 `arch/p0-baseline-001`，与未来软件发布标签分离。

License: [MIT](LICENSE). 开源许可不意味着生产验收已完成。

## 自托管与 AI 接入设计扩展

部署目标覆盖个人电脑、NAS、VPS 和独立服务器，采用 desktop/container/server/postgres Profile。安装助手与记忆客户端分权；无模型模式无需第三方 LLM。符合未来许可条件的个人非商业使用永久免费、不限个人设备、无需官方注册/激活/周期联网或上传真实记忆；当前仓库仍为 MIT。

- [部署与七命令 CLI 契约](docs/DEPLOYMENT_CONTRACT.md)
- [REST、MCP、SDK 与合成接入](docs/AGENT_INTEGRATION.md)
- [自托管安全与恢复](docs/SELF_HOSTED_SECURITY.md)
- [设计检查与未来运行验收](docs/SELF_HOSTED_ACCEPTANCE.md)
- [ADR-008](docs/adr/008-self-hosting-agent-native.md) 与 [机器契约](contracts/deployment/README.md)

这些是架构、接口与合成测试，当前没有可安装发行包、CLI、服务或生产部署；CI 通过不等于实机部署通过。授权 PR #2 仍未实施。

## P0-SC 核心安全边界收敛（Proposal）

本轮只提案收敛 R1/R14，待独立审查：[删除范围](docs/DELETION_SCOPE.md)、[Payload 分类与加密](docs/PAYLOAD_PROTECTION.md)、[契约与 P1 运行门槛](docs/P0_SECURITY_ACCEPTANCE.md)、[ADR-010](docs/adr/010-deletion-scope-boundary.md) / [ADR-011](docs/adr/011-payload-protection.md)。Space 显式图删除仍须完成全部受控副本；跨 Space 来源/主体全面擦除明确拒绝。客户端 synthetic 不豁免全部生产内容加密，NoModel 可使用安全离线 key backend。当前只有设计与纯合成契约测试，无生产删除或密码实现。

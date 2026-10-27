# ADR-006 Repository baseline and verification

Status: Accepted (repository setup) · Date: 2026-10-28

候选：保持无许可证（无法明确开源复用许可）/MIT（简洁宽松）/Apache-2.0（额外专利条款与文本）。本轮选择 MIT 作为初始开源许可；后续变更必须审查贡献者权利与兼容性，不能任意重许可已有贡献。

CI 使用固定 commit 的 GitHub Actions、固定 Python 直接及传递依赖锁、固定 Gitleaks 二进制 SHA256。依赖锁未提供 wheel hash 或供应链认证，后续需评估带 hash 的跨平台 lock。Python 3.12 CI 与本地 3.14 都执行相同契约检查。

初次空仓 main 推送之后建立保护：强制 PR + strict baseline check，管理员受约束，不要求独立 reviewer（当前仅一维护者），不能视为完成独立架构审查。采用 arch/p0-baseline-001 annotated tag，与软件版本分开。兼容影响：后续所有变更走 PR，更新依赖也必须通过检查并核对远端。

验证：API 读取保护与 Actions conclusion，服务器读取 main 和 tag SHA 对比本地，合成泄密反例以及全 Git 历史扫描通过。bootstrap 前保护不生效的窗口明示记录；不得用 force/no-verify 补救。

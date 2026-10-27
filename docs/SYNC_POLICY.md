# Git 同步硬标准

从项目创建起长期有效。任何增删改名都必须形成可追溯提交并同步。检查前先读 AGENTS.md，记录 branch、status、origin，发现未知文件或他人变更不覆盖。

## 每批操作

1. `git status --short --branch`、`git remote -v`；确认账户与目标仓库。
2. 小范围实施；运行 README 全部检查和变更对应的验收测试。
3. `git diff --check`；检查 staged 完整 diff 和路径；运行安全扫描（仅报告文件和类别，不打印敏感值）。
4. 明确添加路径，Commit 描述实际变更；不跳过 hook。
5. 初次空仓基线可直接推送 main；保护建立后正常变更走 PR，所有必需 CI 成功才合并。无独立 reviewer 时不伪造审查。
6. `git push` 后，`git ls-remote origin refs/heads/<branch>` 与 `git rev-parse HEAD` 比较。合并后 fetch 并快进到 main，再核对 main。
7. 标签独立 push，读取 `refs/tags/arch/p0-baseline-001^{}` 对比被标记的提交。不得重写已有标签。
8. 同时核验工作区干净、仓库公开状态、默认分支、CI 状态及保护实际配置。

## 异常恢复

认证/权限/网络错误：立即报告具体错误和 PARTIAL/UNSYNCED，保留本地提交，不换账户绕过、不泄露凭据。404 只说明当前身份未找到；创建冲突立即停止。

Non-fast-forward：fetch → 阅读分叉 diff → 在工作分支解决冲突 → 重跑测试 → 新提交 → PR；不得 force push。未知文件、破坏性操作、敏感公开风险需停止并向用户说明。

提交后发现敏感信息：停止发布；撤销凭据并报告暴露；公开历史清理需用户批准，普通删除提交无法撤销泄漏。不要把秘密再次写入工单或日志。

## 保护与验收

目标：main 要求 PR、严格的 `baseline` 状态检查、管理员同样受约束，禁止 force push 和分支删除；初期无最低 reviewer 数，不能等同独立审查。API 读取保护结果；权限不支持时记录错误，不声称已强制。

本地规则是协作规范；远端保护仅限制其配置覆盖的操作，仓库管理员仍可更改配置。定期读取保护；模拟无 CI 的 PR 应不可合并，避免向 main 实际推送破坏性探针。

同步成功定义：必要检查通过 + CI 成功 + clean 工作区 + 本地 HEAD 等于读取的远端 main SHA。仅有 push 输出或绿色本地测试不足以报告成功。

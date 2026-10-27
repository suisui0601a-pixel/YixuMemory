# P0-Baseline-001 文件与验证清单

软件开发版本 `0.0.0-p0`；本轮只交付设计、数据契约和检查框架。初次本地验证：41 项 pytest 通过，mypy 检查 4 个文件通过，ruff 通过，补充安全扫描无发现，Gitleaks 8.30.1 staged 扫描无发现，git diff --cached --check 通过。测试使用合成数据与临时 SQLite，不连接模型。

GitHub Actions 将用 Python 3.12 独立复验。本地使用 Python 3.14.6、Git 2.55.0.windows.3、gh 2.97.0；工具版本只记录事实，不表示最低版本要求。Gitleaks 发布归档 SHA256 与 Actions commit 已从各官方仓库核验；依赖 lock 未包含下载 hash。

远端 SHA、CI 结论、保护结果、标签被标记提交必须通过 GitHub/ls-remote 在交付时读取；不能在本文件内记录自身最终 commit SHA。初次 bootstrap 推送前无分支保护，此后配置严格 baseline check 与 PR 要求。

## 文件清单

- `.gitattributes`
- `.github/workflows/baseline.yml`
- `.gitignore`
- `AGENTS.md`
- `LICENSE`
- `README.md`
- `SECURITY.md`
- `VERSION`
- `checks/__init__.py`
- `checks/security_scan.py`
- `contracts/README.md`
- `contracts/jsonschema/event.schema.json`
- `contracts/jsonschema/evidence.schema.json`
- `contracts/jsonschema/memory.schema.json`
- `contracts/jsonschema/revision.schema.json`
- `contracts/openapi/openapi.json`
- `docs/ARCHITECTURE.md`
- `docs/COMPATIBILITY.md`
- `docs/CONSISTENCY.md`
- `docs/DOMAIN_MODEL.md`
- `docs/MEMORY_LIFECYCLE.md`
- `docs/P0_ACCEPTANCE.md`
- `docs/RISK_REGISTER.md`
- `docs/SECURITY_MODEL.md`
- `docs/SEQUENCES.md`
- `docs/SYNC_POLICY.md`
- `docs/adr/001-core.md`
- `docs/adr/002-transactions.md`
- `docs/adr/003-erasure.md`
- `docs/adr/004-isolation.md`
- `docs/adr/005-contracts.md`
- `docs/adr/006-repository.md`
- `docs/adr/README.md`
- `pyproject.toml`
- `requirements-dev.lock.txt`
- `requirements-dev.txt`
- `tests/conftest.py`
- `tests/test_contracts.py`
- `tests/test_sqlite_probes.py`
- `docs/BASELINE_MANIFEST.md`

## 尚未通过的生产门槛

见 P0_ACCEPTANCE 中 P1-01 至 P1-11 与 RISK_REGISTER。特别是双后端端口套件、真实敏感数据擦除、备份恢复、完整授权矩阵、模型投毒防御均未实现。独立审查未完成，CI 不能代替独立审查。

## 干净检出修复验证

首个远端运行暴露 work 父目录不存在的问题，修复为 pytest session 启动时创建仓库内 work 目录。正常工作区与无预先 work 的全新 Git archive 均通过 41 项测试；mypy 现在覆盖 5 个源文件。此修复经 PR 的 baseline 检查后合并；早期失败运行保留作为可追溯记录。

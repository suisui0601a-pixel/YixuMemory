# ADR-001 核心与六层

Status: Accepted (design only) · Date: 2026-10-28

候选：框架内插件（迭代快、强耦合）/独立 hexagonal 内核（更多接口、易替换）。选择六层与 Ports，数据库和模型只在 adapter；无模型不调用网络。兼容影响：SDK 仅依赖 DTO。验证：依赖导入扫描、NoModel 下阻断网络仍可人工附证 CRUD；P0 暂无引擎，列为 P1-01。

# COD-007 Runtime Adapter Foundation

## Goal

建立 Agent OS Runtime 边界基础，为未来 Agent 执行能力提供稳定抽象。

本阶段只建立 Runtime Domain Boundary，不接入任何真实模型或执行环境。

## References

必须遵守：

- docs/architecture/agent-os-architecture-v1.md
- docs/architecture/agent-domain-model.md
- docs/architecture/ADR-003-agent-os-architecture-boundaries.md

## Scope

实现：

- Runtime 概念定义
- Runtime Adapter 抽象接口
- Runtime 生命周期模型
- Execution Context 基础结构（仅定义，不执行）
- Agent 与 Runtime 之间的绑定边界设计

## Design Rules

必须保持：

- Agent != Runtime
- Runtime != Provider
- Runtime != Model
- Runtime 不负责权限判断
- Runtime 不包含业务 Workflow

## Allowed

可以实现：

- Rust domain types
- trait/interface abstraction
- repository/service skeleton
- validation rules
- unit tests

## Forbidden

禁止：

- Claude Runtime 实现
- OpenAI Runtime 实现
- Gemini Runtime 实现
- Model 调用
- Provider Gateway
- Permission Engine
- Workflow Execution

## Acceptance Criteria

完成后必须满足：

- Runtime Domain 独立存在
- Agent Domain 不依赖具体 Runtime
- Runtime Adapter 可扩展
- 不影响现有 Agent Registry
- 全量测试通过
- main 保持 clean

## Delivery

提交：

- commit main
- commit hash
- 修改文件列表
- 测试结果
- 架构影响说明

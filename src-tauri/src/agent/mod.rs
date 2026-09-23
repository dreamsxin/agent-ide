pub mod approval;
pub mod diff_apply;
pub mod diff_gen;
pub mod events;
pub mod executor;
/// 撤不回的外部动作记录的落盘面。单独一个模块，因为它是那类动作唯一的补偿。
pub mod external_log;

pub mod multi_agent;
pub mod orchestrator;
pub mod planner;
/// 会话历史的落盘面：没有它，"新建会话 / 回到历史会话"就无从表达。
pub mod session_store;
pub mod state_machine;
/// 把一件子任务交给一个只读子 Agent：它的提示词、边界、交回来的形状。
pub mod subagent;
pub mod task_shape;
pub mod workspace_tools;

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
pub mod state_machine;
pub mod task_shape;
pub mod workspace_tools;

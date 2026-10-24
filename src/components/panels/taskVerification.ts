import type { MessageKey } from "../../i18n/messages";

export interface VerificationReport {
  failed: number;
  skipped: string[];
  repairPrompt: string | null;
  results: { command: string; exitCode: number | null }[];
}

export interface RepairWorkspaceReport {
  iterations: number;
  stopReason: string;
  checksFailed: boolean;
  results: { command: string; exitCode: number | null }[];
}

/**
 * 状态行有两种来源，必须分开表示。
 *
 * 我们自己的话是键加参数，语言跟着界面走；后端的话（"requires Auto mode" 那类拒绝理由）
 * 只能原样显示 —— 它是英文，但把它套进一条中文模板里会变成半句翻译半句原文，而用户要的
 * 恰好是那句原文。前端也不许在这里兜成"修复失败"：那样就看不出该去切模式。
 */
export type StatusLine =
  | { kind: "message"; key: MessageKey; params?: Record<string, string | number> }
  | { kind: "raw"; text: string };

/**
 * 一次全量检查的结果 → 文案键。
 *
 * 返回键而不是句子：跳过几项、几项没过，这些数字的位置在两种语言里不一样，拼字符串就
 * 把英文语序写进了逻辑里。四个键而不是"主句 + 后缀"，因为"跳过 N 项"插在句中哪个位置
 * 也是语言的事。
 */
export function verificationStatus(report: VerificationReport): StatusLine {
  const total = report.results.length;
  const skipped = report.skipped.length;
  if (report.failed === 0) {
    return skipped > 0
      ? { kind: "message", key: "tasks.verify.passed.skipped", params: { count: total, skipped } }
      : { kind: "message", key: "tasks.verify.passed", params: { count: total } };
  }
  return skipped > 0
    ? {
        kind: "message",
        key: "tasks.verify.failed.skipped",
        params: { failed: report.failed, total, skipped },
      }
    : { kind: "message", key: "tasks.verify.failed", params: { failed: report.failed, total } };
}

/**
 * 有界自动修复的结果 → 文案键。
 *
 * `stopReason` 是后端给的英文短语，作为参数原样带过去：那是"为什么停"的唯一信息，
 * 丢掉它用户只知道工作区变了、不知道 Agent 试了几次为什么放弃。后端文案的翻译是另一件事。
 */
export function repairStatus(report: RepairWorkspaceReport): StatusLine {
  return {
    kind: "message",
    key: report.checksFailed ? "tasks.repair.gaveUp" : "tasks.repair.passed",
    params: { rounds: report.iterations, reason: report.stopReason },
  };
}

/** 后端的拒绝或报错原样上屏。两个按钮都会走到这里，所以只写一份 */
export function backendStatus(error: unknown): StatusLine {
  return { kind: "raw", text: error instanceof Error ? error.message : String(error) };
}

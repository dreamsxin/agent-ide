/**
 * Changes 标签上的角标该显示什么。
 *
 * 单独成纯函数，因为这里有一个判断而不只是格式化：一次**只做了外部动作**的运行
 * （比如只打开了页面、只列了窗口）不产生任何待审 diff，于是在角标只数 diff 的版本里
 * 它在界面上完全没有痕迹 —— 用户必须先怀疑什么、才会想到去点开 Changes 看那份记录。
 * 而那份记录是这个能力唯一的补偿，没人指向它的时候它就比声称的弱。
 *
 * 两者不合并成一个数字：待审改动是"你可以决定要不要"，外部动作是"已经发生、撤不回"。
 * 用同一个计数会把这两件事说成一件。
 */
export type ChangesBadgeTone = "pending" | "external";

export interface ChangesBadge {
  text: string;
  tone: ChangesBadgeTone;
  /** 给 `title` 用的说明，点开之前就该知道角标在说什么 */
  hint: string;
}

/**
 * @param pendingChanges 待审的 diff 数
 * @param performedExternal 已经发生、撤不回的外部动作数（不含被拒/失败/被停的）
 */
export function changesBadge(
  pendingChanges: number,
  performedExternal: number
): ChangesBadge | null {
  if (pendingChanges > 0) {
    return {
      text: String(pendingChanges),
      tone: "pending",
      hint: `${pendingChanges} change(s) waiting for review`,
    };
  }
  if (performedExternal > 0) {
    // 没有待审改动，但外面被动过：用另一种色调，因为这不是"等你决定"，而是"已经发生"
    return {
      text: String(performedExternal),
      tone: "external",
      hint: `${performedExternal} external action(s) that cannot be undone`,
    };
  }
  return null;
}

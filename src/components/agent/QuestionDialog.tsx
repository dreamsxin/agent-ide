import { useEffect, useState } from "react";
import { useAgentStore } from "../../stores/useAgentStore";

/**
 * 模型问用户一道选择题的提示框。
 *
 * 后端有一次工具调用正**挂在这里等**：`agent-question-requested` 把它放进
 * `pendingQuestion`，选一项或自己写一句把答案送回去。不答，两分钟后这次调用会带着
 * "没人回答"回到模型，它只能自己判断 —— 所以这个框不是装饰，但也不会卡死运行。
 *
 * 自由输入永远在：模型给的选项是它想到的几种，不是全部。只能在其中选等于让模型的
 * 想象力当成用户的全部选项。
 *
 * Esc 走**不回答**而不是随便选一项：随手关掉绝不能变成一个被当成用户偏好的答案。
 */
export default function QuestionDialog() {
  const pendingQuestion = useAgentStore((s) => s.pendingQuestion);
  const answerQuestion = useAgentStore((s) => s.answerQuestion);
  const dismissQuestion = useAgentStore((s) => s.dismissQuestion);
  const [custom, setCustom] = useState("");

  // 换了一道题就清掉上一道题里写了一半的答案：留着它等于把上一个问题的回答提交给
  // 这一个问题
  useEffect(() => {
    setCustom("");
  }, [pendingQuestion?.id]);

  useEffect(() => {
    if (!pendingQuestion) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void dismissQuestion();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [pendingQuestion, dismissQuestion]);

  if (!pendingQuestion) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="agent-question-title"
        className="w-full max-w-sm rounded-lg border border-surface-border bg-surface-panel shadow-xl"
      >
        <div className="flex items-center gap-2 border-b border-surface-border px-4 py-3">
          <span className="text-lg" aria-hidden="true">
            {"\u{2753}"}
          </span>
          <div>
            <div id="agent-question-title" className="text-sm font-semibold text-surface-text">
              The Agent needs a decision
            </div>
            <div className="text-[11px] text-surface-muted">Your answer, not an approval</div>
          </div>
        </div>

        <div className="px-4 py-3">
          <p className="text-xs text-surface-text leading-relaxed">{pendingQuestion.question}</p>
          <div className="mt-3 flex flex-col gap-1.5">
            {pendingQuestion.options.map((option, index) => (
              <button
                key={option}
                // 焦点落在第一项：它是模型认为最可能的答案，而键盘用户不该先 Tab 过整个列表
                autoFocus={index === 0}
                onClick={() => void answerQuestion(option)}
                className="rounded border border-surface-border px-3 py-1.5 text-left text-xs text-surface-text hover:bg-surface-border/30 transition-colors"
              >
                {option}
              </button>
            ))}
          </div>
          <form
            className="mt-3 flex gap-1.5"
            onSubmit={(event) => {
              event.preventDefault();
              void answerQuestion(custom);
            }}
          >
            <input
              value={custom}
              onChange={(event) => setCustom(event.target.value)}
              placeholder="Or type your own answer"
              aria-label="Your own answer"
              className="flex-1 rounded border border-surface-border bg-surface-base px-2 py-1.5 text-xs text-surface-text"
            />
            <button
              type="submit"
              // 空输入不提交：后端会拒，而界面上"提交了空答案"看起来像成功
              disabled={!custom.trim()}
              className="rounded bg-accent-blue px-3 py-1.5 text-xs text-white font-medium hover:bg-accent-blue/80 disabled:opacity-40 transition-colors"
            >
              Send
            </button>
          </form>
        </div>

        <div className="flex justify-end border-t border-surface-border px-4 py-3">
          <button
            onClick={() => void dismissQuestion()}
            className="rounded border border-surface-border px-4 py-1.5 text-xs text-surface-muted hover:bg-surface-border/30 transition-colors"
          >
            Don't answer
          </button>
        </div>
      </div>
    </div>
  );
}

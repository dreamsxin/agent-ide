import React from "react";

export default function App() {
  return (
    <div className="h-screen grid grid-rows-[48px_1fr_240px] grid-cols-[240px_1fr_360px] bg-[#0D1117] text-[#E6EDF3]">
      <TopBar />
      <LeftPanel />
      <Editor />
      <AgentPanel />
      <BottomPanel />
    </div>
  );
}

function TopBar() {
  return (
    <div className="col-span-3 flex items-center justify-between px-4 border-b border-[#30363D] bg-[#161B22]">
      <div className="flex items-center gap-3">
        <div className="font-bold">Agent IDE</div>
        <div className="text-sm text-[#8B949E]">my-project</div>
      </div>

      <ModeSwitch />

      <div className="flex items-center gap-3">
        <StatusDot status="acting" />
        <button className="px-3 py-1 bg-blue-600 rounded">Run</button>
        <button className="px-3 py-1 bg-gray-700 rounded">Stop</button>
      </div>
    </div>
  );
}

function ModeSwitch() {
  return (
    <div className="flex bg-[#0D1117] rounded p-1 border border-[#30363D]">
      {["Suggest", "Edit", "Auto"].map((m) => (
        <button
          key={m}
          className="px-3 py-1 text-sm rounded hover:bg-[#30363D]"
        >
          {m}
        </button>
      ))}
    </div>
  );
}

function StatusDot({ status }) {
  const color =
    status === "acting"
      ? "bg-blue-500"
      : status === "thinking"
      ? "bg-purple-500"
      : "bg-gray-500";

  return <div className={`w-2 h-2 rounded-full ${color}`} />;
}

function LeftPanel() {
  return (
    <div className="border-r border-[#30363D] bg-[#161B22] p-2 text-sm">
      <div className="mb-2 font-semibold">Explorer</div>
      <div className="text-[#8B949E]">src/</div>
      <div className="ml-2">auth.js</div>
      <div className="ml-2">index.js</div>
    </div>
  );
}

function Editor() {
  return (
    <div className="relative">
      <CodeLayer />
      <InlineSuggestionLayer />
      <DiffOverlayLayer />
      <IntentLayer />
    </div>
  );
}

function CodeLayer() {
  return (
    <pre className="p-4 text-sm font-mono">
{`function login(req, res) {
  const { email, password } = req.body;
}`}
    </pre>
  );
}

function InlineSuggestionLayer() {
  return (
    <div className="absolute top-16 left-4 text-gray-500 italic">
      // AI suggestion...
    </div>
  );
}

function DiffOverlayLayer() {
  return (
    <div className="absolute top-32 left-0 right-0">
      <div className="bg-green-900/30 p-2 border-l-4 border-green-600">
        + const token = generateToken()
      </div>
      <div className="bg-red-900/30 p-2 border-l-4 border-red-600">
        - old password logic
      </div>
    </div>
  );
}

function IntentLayer() {
  return (
    <div className="absolute bottom-4 right-4 bg-purple-600 text-white text-xs px-2 py-1 rounded">
      💡 Optimize this
    </div>
  );
}

function AgentPanel() {
  return (
    <div className="border-l border-[#30363D] bg-[#161B22] flex flex-col">
      <div className="flex border-b border-[#30363D]">
        {["Chat", "Tasks", "Diff"].map((tab) => (
          <div key={tab} className="px-4 py-2 text-sm">
            {tab}
          </div>
        ))}
      </div>

      <div className="p-3 flex-1 overflow-auto">
        <TaskItem title="Create auth.js" status="done" />
        <TaskItem title="Add JWT" status="doing" />
        <TaskItem title="Write tests" status="todo" />
      </div>

      <div className="p-2 border-t border-[#30363D]">
        <input
          className="w-full bg-[#0D1117] p-2 rounded text-sm"
          placeholder="Ask Agent..."
        />
      </div>
    </div>
  );
}

function TaskItem({ title, status }) {
  const color =
    status === "done"
      ? "text-green-500"
      : status === "doing"
      ? "text-blue-500"
      : "text-gray-500";

  return (
    <div className={`text-sm mb-2 ${color}`}>
      [{status}] {title}
    </div>
  );
}

function BottomPanel() {
  return (
    <div className="col-span-3 border-t border-[#30363D] bg-black p-2 text-xs font-mono">
      <div>$ npm test</div>
      <div className="text-red-500">FAIL login test</div>
    </div>
  );
}

import { Component, type ErrorInfo, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
  fallbackTitle?: string;
}

interface ErrorBoundaryState {
  error: Error | null;
}

export default class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[ErrorBoundary]", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;

    return (
      <div className="flex h-full items-center justify-center bg-surface-base p-4 text-xs text-surface-text">
        <div className="max-w-xl rounded border border-diff-remove/40 bg-surface-panel p-3">
          <div className="font-semibold text-diff-remove">
            {this.props.fallbackTitle ?? "Panel failed to render"}
          </div>
          <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-surface-muted">
            {this.state.error.message}
          </pre>
          <button
            onClick={() => this.setState({ error: null })}
            className="mt-3 rounded border border-surface-border px-2 py-1 text-[11px] text-surface-muted hover:text-surface-text"
          >
            Retry
          </button>
        </div>
      </div>
    );
  }
}

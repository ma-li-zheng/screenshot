import { Component, type ReactNode } from "react";

/** 界面出错时显示错误信息，而不是一片空白 */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="crash">
        <div className="crash-title">界面出错了</div>
        <pre className="crash-msg">{error.message || String(error)}</pre>
        <button onClick={() => location.reload()}>重新加载</button>
      </div>
    );
  }
}

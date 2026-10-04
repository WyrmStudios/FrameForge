import { Component, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  err: string | null;
}

export default class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { err: null };
  }

  static getDerivedStateFromError(error: Error) {
    return { err: error.message };
  }

  render() {
    if (this.state.err) {
      return <div className="whitespace-pre-wrap p-6 font-mono text-danger">
        <strong>Render error:</strong>{"\n"}{this.state.err}
      </div>;
    }
    return this.props.children;
  }
}

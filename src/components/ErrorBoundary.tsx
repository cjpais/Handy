import { Component, type ErrorInfo, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
  context: string;
}

interface ErrorBoundaryState {
  failed: boolean;
}

/** Prevents a non-critical UI subtree from blanking the entire app. */
export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error(
      `Error rendering ${this.props.context}:`,
      error,
      info.componentStack,
    );
  }

  render(): ReactNode {
    if (this.state.failed) return null;
    return this.props.children;
  }
}

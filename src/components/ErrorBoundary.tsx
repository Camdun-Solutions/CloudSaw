// Top-level React error boundary. Catches render-time exceptions in any
// sub-tree and surfaces the Contract 12A error reporting dialog so the
// user can file a bug report (with the failing message pre-filled into
// the notes) before they reload. A fallback panel always offers
// "Reload" so the user is never wedged.

import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = {
  children: ReactNode;
  /** Render prop for the dialog. Receives the captured error message
   * and a `clear` callback so the boundary can stop rendering the
   * fallback once the user dismisses. */
  fallback: (state: {
    errorMessage: string;
    clear: () => void;
  }) => ReactNode;
};

type State = { errorMessage: string | null };

export default class ErrorBoundary extends Component<Props, State> {
  state: State = { errorMessage: null };

  static getDerivedStateFromError(error: unknown): State {
    const message =
      error instanceof Error ? error.message : "An unexpected error occurred.";
    return { errorMessage: message };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    // Surface to the dev console so the user can also report through
    // their browser dev tools if they prefer. Production redaction is
    // handled by the backend bundle builder before anything is sent
    // anywhere — the boundary only displays a short message.
    // eslint-disable-next-line no-console
    console.error("CloudSaw render error:", error, info);
  }

  render() {
    if (this.state.errorMessage) {
      return this.props.fallback({
        errorMessage: this.state.errorMessage,
        clear: () => this.setState({ errorMessage: null }),
      });
    }
    return this.props.children;
  }
}

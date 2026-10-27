// A React error boundary that writes the error to the central log (`specs/039`) and then lets it carry on to Ink's own
// boundary, so the console still stops, restores the terminal and prints the error exactly as it did before.
import React from 'react';
import { logCrash } from './elog.ts';

type Props = { children?: React.ReactNode };
type State = { error: unknown };

export class ErrorBoundary extends React.Component<Props, State> {
  state: State = { error: null };

  // Logged here, not in `componentDidCatch`: this boundary rethrows, so React never commits it and that hook would not run.
  // `logCrash` writes once per error object, so React calling this twice (it may) is harmless.
  static getDerivedStateFromError(error: unknown): State {
    logCrash('render_error', error, { view: 'render' });
    return { error };
  }

  render(): React.ReactNode {
    if (this.state.error !== null) throw this.state.error; // rethrown: the next boundary (Ink's) handles it as before
    return this.props.children;
  }
}

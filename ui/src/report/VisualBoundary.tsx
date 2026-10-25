// Keeps a visual that fails to render from taking the page down: it shows the error in
// its own frame, with a retry, and starts again when the visual changes.
import { AlertTriangle } from "lucide-react";
import { Component, type ReactNode } from "react";
import { t } from "../i18n";

interface Props {
  /** Changes when the visual does: the boundary resets. */
  resetKey: string;
  children: ReactNode;
}

export class VisualBoundary extends Component<Props, { error: Error | null; key: string }> {
  state = { error: null as Error | null, key: this.props.resetKey };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  static getDerivedStateFromProps(p: Props, s: { error: Error | null; key: string }) {
    return p.resetKey !== s.key ? { error: null, key: p.resetKey } : null;
  }

  componentDidCatch(error: Error) {
    console.error("visual failed to render:", error);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="visual-message is-error" role="alert">
        <AlertTriangle size={18} />
        <span>{t("Couldn't display the visual")}</span>
        <span className="visual-error-detail">{this.state.error.message}</span>
        <button type="button" className="link-btn" onClick={() => this.setState({ error: null })}>
          {t("Try again")}
        </button>
      </div>
    );
  }
}

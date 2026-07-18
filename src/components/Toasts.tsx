import { AlertTriangle, Check, Info, X } from "lucide-react";

import { tr } from "../i18n";
import { useKomaStore, type ToastTone } from "../store/koma";

function ToneIcon({ tone }: { tone: ToastTone }) {
  if (tone === "success") return <Check size={16} />;
  if (tone === "danger" || tone === "warning") {
    return <AlertTriangle size={16} />;
  }
  return <Info size={16} />;
}

export function Toasts() {
  const toasts = useKomaStore((state) => state.toasts);
  const dismiss = useKomaStore((state) => state.dismissToast);
  return (
    <div
      className="toast-region"
      role="region"
      aria-live="polite"
      aria-label={tr("Notifications")}
    >
      {toasts.map((toast) => (
        <div className={`toast tone-${toast.tone}`} key={toast.id}>
          <span className="toast-icon">
            <ToneIcon tone={toast.tone} />
          </span>
          <div>
            <strong>{toast.title}</strong>
            {toast.detail !== null && <p>{toast.detail}</p>}
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label={tr("Dismiss notification")}
            onClick={() => dismiss(toast.id)}
          >
            <X size={15} />
          </button>
        </div>
      ))}
    </div>
  );
}

import { Download, LoaderCircle, X } from "lucide-react";
import { useEffect, useState } from "react";

import { tr } from "../i18n";
import { backend } from "../lib/backend";
import type { ImportEvent } from "../types";

interface DownloadActivity {
  id: string;
  title: string;
  detail: string;
  completed: number;
  total: number;
  cancelling: boolean;
}

function updateActivity(
  current: DownloadActivity | undefined,
  event: ImportEvent,
  id: string,
): DownloadActivity | null {
  const activity = current ?? {
    id,
    title: tr("Preparing download"),
    detail: tr("Checking source…"),
    completed: 0,
    total: 0,
    cancelling: false,
  };

  switch (event.kind) {
    case "checking":
      return activity;
    case "eligible":
      return { ...activity, detail: tr("Getting pages ready…") };
    case "discovered":
      return {
        ...activity,
        title: event.title,
        detail: event.volume,
        total: event.pageCount,
      };
    case "downloading":
      return {
        ...activity,
        completed: event.completed,
        total: event.total,
        detail: tr("{{completed}} of {{total}} pages", {
          completed: event.completed,
          total: event.total,
        }),
      };
    case "recovering":
      return {
        ...activity,
        detail: tr("Retrying {{count}} pages", { count: event.failedPages }),
      };
    case "packaging":
      return {
        ...activity,
        completed: activity.total,
        detail: tr("Finishing download…"),
      };
    case "completed":
      return null;
  }
}

export function DownloadTray() {
  const [activities, setActivities] = useState<DownloadActivity[]>([]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void backend
      .onImportEvent((event, jobId) => {
        if (!active) return;
        setActivities((items) => {
          const current = items.find((item) => item.id === jobId);
          const next = updateActivity(current, event, jobId);
          if (next === null) return items.filter((item) => item.id !== jobId);
          return current === undefined
            ? [...items, next]
            : items.map((item) => (item.id === jobId ? next : item));
        });
      })
      .then((next) => {
        if (active) unlisten = next;
        else next();
      });

    const finish = (event: Event) => {
      const jobId = (event as CustomEvent<string>).detail;
      setActivities((items) => items.filter((item) => item.id !== jobId));
    };
    window.addEventListener("koma:import-finished", finish);
    return () => {
      active = false;
      unlisten?.();
      window.removeEventListener("koma:import-finished", finish);
    };
  }, []);

  const cancel = async (jobId: string) => {
    setActivities((items) =>
      items.map((item) =>
        item.id === jobId
          ? { ...item, cancelling: true, detail: tr("Cancelling…") }
          : item,
      ),
    );
    try {
      await backend.cancelImport(jobId);
    } finally {
      setActivities((items) => items.filter((item) => item.id !== jobId));
    }
  };

  if (activities.length === 0) return null;

  return (
    <aside
      className="download-tray"
      aria-label={tr("Downloads")}
      aria-live="polite"
    >
      {activities.map((activity) => {
        const progress =
          activity.total > 0
            ? Math.min(100, (activity.completed / activity.total) * 100)
            : null;
        return (
          <div className="download-activity" key={activity.id}>
            <span className="download-activity-icon" aria-hidden="true">
              {activity.cancelling ? (
                <LoaderCircle className="spin" size={17} />
              ) : (
                <Download size={17} />
              )}
            </span>
            <div className="download-activity-copy">
              <strong>{activity.title}</strong>
              <span>{activity.detail}</span>
            </div>
            <button
              type="button"
              className="download-activity-cancel"
              onClick={() => void cancel(activity.id)}
              disabled={activity.cancelling}
              aria-label={tr("Cancel download")}
              title={tr("Cancel download")}
            >
              <X size={16} />
            </button>
            <div
              className={`download-activity-progress${progress === null ? " indeterminate" : ""}`}
              role="progressbar"
              aria-label={tr("Download progress")}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={
                progress === null ? undefined : Math.round(progress)
              }
            >
              <span
                style={
                  progress === null ? undefined : { width: `${progress}%` }
                }
              />
            </div>
          </div>
        );
      })}
    </aside>
  );
}

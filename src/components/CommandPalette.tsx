import * as Dialog from "@radix-ui/react-dialog";
import {
  Archive,
  BookOpen,
  Clock3,
  FileArchive,
  FolderOpen,
  Heart,
  Home,
  Import,
  LibraryBig,
  Moon,
  Search,
  Settings,
  Sun,
  Wrench,
  X,
} from "lucide-react";
import { type KeyboardEvent as ReactKeyboardEvent, useState } from "react";

import { tr } from "../i18n";
import { useKomaStore } from "../store/koma";

interface Command {
  id: string;
  label: string;
  detail: string;
  icon: typeof Search;
  shortcut?: string;
  run: () => void;
}

export function CommandPalette() {
  const open = useKomaStore((state) => state.commandOpen);
  const setOpen = useKomaStore((state) => state.setCommandOpen);
  const setRoute = useKomaStore((state) => state.setRoute);
  const setImportOpen = useKomaStore((state) => state.setImportOpen);
  const addFiles = useKomaStore((state) => state.addFiles);
  const addFolder = useKomaStore((state) => state.addFolder);
  const exportBackup = useKomaStore((state) => state.exportBackup);
  const setTheme = useKomaStore((state) => state.setTheme);
  const setToolsItemId = useKomaStore((state) => state.setToolsItemId);
  const selectedId = useKomaStore((state) => state.selectedId);
  const selected = useKomaStore((state) =>
    state.items.find((item) => item.id === selectedId),
  );
  const openBook = useKomaStore((state) => state.openBook);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const modifier = /mac|iphone|ipad/i.test(navigator.platform) ? "⌘" : "Ctrl+";

  const commands: Command[] = [
      ...(selected !== undefined
        ? [
            {
              id: "open-selected",
              label: tr("Read {{title}}", { title: selected.title }),
              detail: tr("Open the selected publication"),
              icon: BookOpen,
              shortcut: "↵",
              run: () => void openBook(selected),
            },
            {
              id: "tools-selected",
              label: tr("Inspect {{title}}", { title: selected.title }),
              detail: tr("Inspect, edit, convert, or repair"),
              icon: Wrench,
              run: () => setToolsItemId(selected.id),
            },
          ]
        : []),
      {
        id: "add-files",
        label: tr("Add files"),
        detail: tr("CBZ, CBR, CB7, CBT, EPUB, PDF, and image folders"),
        icon: FileArchive,
        shortcut: `${modifier}O`,
        run: () => void addFiles(),
      },
      {
        id: "scan-folder",
        label: tr("Scan folder"),
        detail: tr("Find archives and image folders"),
        icon: FolderOpen,
        run: () => void addFolder(),
      },
      {
        id: "import-link",
        label: tr("Import from link"),
        detail: tr("Use an installed connector"),
        icon: Import,
        run: () => setImportOpen(true),
      },
      {
        id: "home",
        label: tr("Go to Home"),
        detail: tr("Continue reading"),
        icon: Home,
        run: () => setRoute("home"),
      },
      {
        id: "library",
        label: tr("Go to Library"),
        detail: tr("All publications"),
        icon: LibraryBig,
        run: () => setRoute("library"),
      },
      {
        id: "continue",
        label: tr("Go to Continue"),
        detail: tr("In progress"),
        icon: Clock3,
        run: () => setRoute("continue"),
      },
      {
        id: "favorites",
        label: tr("Go to Favorites"),
        detail: tr("Favorite publications"),
        icon: Heart,
        run: () => setRoute("favorites"),
      },
      {
        id: "settings",
        label: tr("Open Settings"),
        detail: tr("App preferences"),
        icon: Settings,
        shortcut: `${modifier},`,
        run: () => setRoute("settings"),
      },
      {
        id: "backup",
        label: tr("Export library backup"),
        detail: tr("Progress, bookmarks, and metadata"),
        icon: Archive,
        run: () => void exportBackup(),
      },
      {
        id: "theme-light",
        label: tr("Use light theme"),
        detail: tr("Light"),
        icon: Sun,
        run: () => setTheme("light"),
      },
      {
        id: "theme-dark",
        label: tr("Use dark theme"),
        detail: tr("Dark"),
        icon: Moon,
        run: () => setTheme("dark"),
      },
    ];

  const needle = query.trim().toLocaleLowerCase();
  const filtered = commands.filter(
    (command) =>
      needle.length === 0 ||
      command.label.toLocaleLowerCase().includes(needle) ||
      command.detail.toLocaleLowerCase().includes(needle),
  );

  const run = (command: Command) => {
    setOpen(false);
    setQuery("");
    setActiveIndex(0);
    command.run();
  };

  const navigate = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (filtered.length === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((current) => (current + 1) % filtered.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex(
        (current) => (current - 1 + filtered.length) % filtered.length,
      );
    } else if (event.key === "Enter") {
      event.preventDefault();
      const command = filtered[Math.min(activeIndex, filtered.length - 1)];
      if (command !== undefined) run(command);
    }
  };

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) {
          setQuery("");
          setActiveIndex(0);
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="command-dialog" aria-describedby={undefined}>
          <Dialog.Title className="sr-only">{tr("Koma commands")}</Dialog.Title>
          <div className="command-search">
            <Search size={18} aria-hidden="true" />
            <input
              autoFocus
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActiveIndex(0);
              }}
              onKeyDown={navigate}
              placeholder={tr("Search commands")}
              aria-label={tr("Search commands")}
              aria-controls="koma-command-results"
              aria-activedescendant={
                filtered[activeIndex] === undefined
                  ? undefined
                  : `koma-command-${filtered[activeIndex].id}`
              }
            />
            <Dialog.Close asChild>
              <button
                type="button"
                className="icon-button"
                aria-label={tr("Close commands")}
              >
                <X size={17} />
              </button>
            </Dialog.Close>
          </div>
          <div className="command-results" id="koma-command-results" role="listbox">
            {filtered.length === 0 ? (
              <div className="command-empty">
                {tr("No command matches “{{query}}”.", { query })}
              </div>
            ) : (
              filtered.map((command, index) => {
                const Icon = command.icon;
                return (
                  <button
                    type="button"
                    className={
                      index === activeIndex ? "command-row is-active" : "command-row"
                    }
                    onClick={() => run(command)}
                    onPointerMove={() => setActiveIndex(index)}
                    key={command.id}
                    id={`koma-command-${command.id}`}
                    role="option"
                    aria-selected={index === activeIndex}
                    tabIndex={-1}
                  >
                    <span className="command-icon">
                      <Icon size={17} />
                    </span>
                    <span>
                      <strong>{command.label}</strong>
                      <small>{command.detail}</small>
                    </span>
                    {command.shortcut !== undefined && (
                      <kbd>{command.shortcut}</kbd>
                    )}
                  </button>
                );
              })
            )}
          </div>
          <div className="command-foot">
            <span>
              <kbd>↑↓</kbd> {tr("Navigate")}
            </span>
            <span>
              <kbd>↵</kbd> {tr("Open")}
            </span>
            <span>
              <kbd>esc</kbd> {tr("Close")}
            </span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import { useKomaStore } from "./store/koma";

const initialState = useKomaStore.getInitialState();

async function renderKoma() {
  render(<App />);
  await screen.findByRole("button", { name: "Continue reading" });
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  useKomaStore.setState(
    {
      ...initialState,
      initialized: false,
      booting: false,
      bootstrap: null,
      items: [],
      route: "home",
      search: "",
      selectedId: null,
      reader: null,
      readerOpeningId: null,
      importOpen: false,
      commandOpen: false,
      toolsItemId: null,
      sidebarOpen: false,
      dropActive: false,
      toasts: [],
      passwordRequest: null,
    },
    true,
  );
});

afterEach(() => {
  cleanup();
});

describe("Koma application workflows", () => {
  it("boots a usable library and navigates through command search", async () => {
    await renderKoma();
    expect(
      screen.getByRole("button", { name: "Continue reading" }),
    ).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const commands = await screen.findByRole("textbox", {
      name: "Search commands",
    });
    await userEvent.type(commands, "settings");
    fireEvent.keyDown(commands, { key: "Enter" });

    expect(
      await screen.findByRole("heading", { name: "Settings", level: 2 }),
    ).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Language" })).toBeInTheDocument();
  });

  it("opens the reader, navigates by keyboard, and saves a page note", async () => {
    await renderKoma();
    await userEvent.click(
      screen.getByRole("button", { name: "Continue reading" }),
    );

    expect(
      await screen.findByLabelText("Reading After the Last Train"),
    ).toBeInTheDocument();
    const slider = screen.getByRole("slider", { name: "Page" });
    expect(slider).toHaveAttribute("aria-valuenow", "18");

    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Reading mode" }),
      "spreads",
    );
    await waitFor(() => {
      expect(useKomaStore.getState().reader?.settings.mode).toBe("spreads");
    });

    fireEvent.keyDown(window, { key: "ArrowRight" });
    await waitFor(() => {
      expect(slider).toHaveAttribute("aria-valuenow", "20");
    });

    fireEvent.keyDown(window, { key: "s" });
    expect(
      await screen.findByRole("heading", { name: "Reader settings" }),
    ).toBeInTheDocument();
    const pageGap = screen.getByRole("checkbox", { name: "Page gap" });
    expect(pageGap).toBeChecked();
    await userEvent.click(pageGap);
    await waitFor(() => {
      expect(useKomaStore.getState().reader?.settings.spreadGapEnabled).toBe(
        false,
      );
    });
    await userEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    await waitFor(() => {
      expect(useKomaStore.getState().reader?.zoom).toBeGreaterThan(1);
    });
    await userEvent.type(screen.getByLabelText("Label"), "Composition");
    await userEvent.type(
      screen.getByLabelText("Note"),
      "Return to the negative space.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save note" }));

    expect(await screen.findByText("Page note saved")).toBeInTheDocument();
    expect(screen.getByText("Composition")).toBeInTheDocument();
  });

  it("requires permission confirmation before packaging an imported volume", async () => {
    await renderKoma();
    await userEvent.click(
      screen.getByRole("button", { name: "Import from link" }),
    );
    const source = screen.getByLabelText("Source link");
    await userEvent.type(
      source,
      "https://mangafire.to/title/70ox7-hatori-to-furuta-no-hinichijou-sahanji/volume/339405",
    );
    await userEvent.click(screen.getByRole("button", { name: "Check link" }));

    expect(await screen.findByText("MangaFire")).toBeInTheDocument();
    const download = screen.getByRole("button", {
      name: "Download and add to Koma",
    });
    expect(download).toBeDisabled();
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: "I have permission to download this work.",
      }),
    );
    expect(download).toBeEnabled();
    await userEvent.click(download);

    expect(await screen.findByText("Done", {}, { timeout: 2_500 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open library" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Open library" }));
    await userEvent.click(
      screen.getByRole("button", { name: "Import from link" }),
    );
    expect(screen.getByLabelText("Source link")).toHaveValue("");
    expect(screen.queryByText("MangaFire")).not.toBeInTheDocument();
  });

  it("opens the publication workbench and proves page inspection results", async () => {
    await renderKoma();
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const commands = await screen.findByRole("textbox", {
      name: "Search commands",
    });
    await userEvent.type(commands, "inspect");
    fireEvent.keyDown(commands, { key: "Enter" });

    expect(
      await screen.findByRole("dialog", {
        name: "Publication tools for After the Last Train",
      }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", {
        name: "No issues found",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText(/42 of 42 pages checked/)).toBeInTheDocument();
  });

  it("windows very large libraries instead of mounting every publication", async () => {
    await renderKoma();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      top: 0,
      right: 960,
      bottom: 720,
      left: 0,
      width: 960,
      height: 720,
      toJSON: () => ({}),
    });
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(960);
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(720);
    const seed = useKomaStore.getState().items[0];
    expect(seed).toBeDefined();
    if (seed === undefined) return;
    const items = Array.from({ length: 20_000 }, (_, index) => ({
      ...seed,
      id: `virtual-publication-${index}`,
      title: `Publication ${index + 1}`,
      path: `/Library/Publication ${index + 1}.cbz`,
    }));
    useKomaStore.setState({
      items,
      route: "library",
      selectedId: items[0]?.id ?? null,
    });

    expect(
      await screen.findByText("20,000 of 20,000 publications"),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(document.querySelectorAll(".virtual-grid-row").length).toBeGreaterThan(0);
      const rendered = screen.getAllByRole("article");
      expect(rendered.length).toBeGreaterThan(0);
      expect(rendered.length).toBeLessThan(100);
    });
  });
});

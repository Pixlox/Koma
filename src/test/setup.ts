import "@testing-library/jest-dom/vitest";

import { vi } from "vitest";

class ResizeObserverStub implements ResizeObserver {
  constructor(private readonly callback: ResizeObserverCallback) {}

  observe(target: Element) {
    this.callback(
      [
        {
          target,
          contentRect: target.getBoundingClientRect(),
        } as ResizeObserverEntry,
      ],
      this,
    );
  }
  unobserve() {}
  disconnect() {}
}

class IntersectionObserverStub implements IntersectionObserver {
  readonly root = null;
  readonly rootMargin = "0px";
  readonly thresholds = [0];

  disconnect() {}
  observe() {}
  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }
  unobserve() {}
}

Object.defineProperty(globalThis, "ResizeObserver", {
  configurable: true,
  value: ResizeObserverStub,
});
Object.defineProperty(globalThis, "IntersectionObserver", {
  configurable: true,
  value: IntersectionObserverStub,
});
Object.defineProperty(window, "matchMedia", {
  configurable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});
Object.defineProperty(navigator, "clipboard", {
  configurable: true,
  value: {
    readText: vi.fn().mockResolvedValue(""),
    writeText: vi.fn().mockResolvedValue(undefined),
  },
});
Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  configurable: true,
  value: vi.fn(),
});
Object.defineProperty(HTMLElement.prototype, "hasPointerCapture", {
  configurable: true,
  value: vi.fn().mockReturnValue(false),
});
Object.defineProperty(HTMLElement.prototype, "setPointerCapture", {
  configurable: true,
  value: vi.fn(),
});
Object.defineProperty(HTMLElement.prototype, "releasePointerCapture", {
  configurable: true,
  value: vi.fn(),
});

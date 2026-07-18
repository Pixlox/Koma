---
name: Koma
description: A precise library frame that disappears around the page.
---

# Design System: Koma

## 1. Overview

**Creative north star: The Vanishing Frame**

Someone opens Koma beside a bright window, continues in a dim room, and wants the artwork rather than the application to set the mood. The library therefore follows the system theme, while the reader always uses a neutral dark surround that neither tints nor competes with a page.

Koma is a restrained product interface: warm paper and ink neutrals, one rare persimmon action color, system typography, tonal separation, and almost no decorative elevation. Its character comes from exact rhythm, the asymmetric Koma mark, crisp book geometry, and controls that withdraw when reading begins.

The interface supports three spatial states:

- Wide desktop and iPad: a 208px sidebar, 64px toolbar, content canvas, and optional 298px inspector.
- Compact tablet: a temporary navigation drawer, condensed toolbar, and the same content hierarchy.
- Phone: a search-first toolbar, full-width work surfaces, and four-item bottom navigation with safe-area padding.

The supported floor is 320 by 480 CSS pixels. Layout transitions occur at 1120px, 760px, and 560px.

## 2. Colors

Koma uses OKLCH so lightness and chroma remain intentional across themes. The color strategy is restrained: the accent should occupy less than ten percent of a screen.

### Light surfaces

- Canvas: `oklch(94.65% 0.0058 84.6)`
- Surface: `oklch(97.06% 0.0057 84.6)`
- Raised surface: `oklch(99.45% 0.0057 84.6)`
- Sidebar: `oklch(91.95% 0.0087 84.6)`
- Ink: `oklch(25.34% 0.0048 67.6)`
- Soft ink: `oklch(40.05% 0.0085 67.6)`
- Muted ink: `oklch(47.85% 0.0122 67.5)`
- Faint ink: `oklch(50.69% 0.0121 67.6)`

### Brand and state

- Persimmon action: `oklch(54.09% 0.1467 33.2)`
- Persimmon hover: `oklch(47.36% 0.1325 32.2)`
- Persimmon tint: `oklch(88.83% 0.0253 39.4)`
- Focus: `oklch(50.85% 0.1413 32.4)`
- Danger: `oklch(51.83% 0.1498 28.8)`
- Success: `oklch(45.84% 0.0678 156.7)`
- Warning: `oklch(46.33% 0.0819 69.9)`

### Reader

- Surround: `oklch(19.08% 0.002 106.6)`
- Reader surface: `oklch(23% 0.0038 106.7)`
- Raised reader surface: `oklch(27.16% 0.0054 106.8)`
- Reader ink: `oklch(95.03% 0.0103 81.8)`
- Reader accent: `oklch(65.46% 0.1562 34.1)`

Dark library surfaces are similarly warm, with canvas lightness at 20.98% and primary ink at 94.45%. State never relies on color alone: icons, labels, shape, and `aria` state carry the same meaning.

## 3. Typography

The product uses one native voice:

```css
-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", Roboto,
Helvetica, Arial, sans-serif
```

Technical values and page numbers use `ui-monospace`, `"SFMono-Regular"`, Consolas, and `monospace`.

- Empty-state display: 35px, weight 710, tracking `-0.048em`
- Feature title: responsive 25px to 38px, weight 700
- Section title: 19px to 24px, weight 680
- Publication title: 12px to 14px, weight 650
- Interface body: 13px to 14px
- Compact labels: 10px to 12px, weight 620 to 700
- Technical labels: 8px to 10px, monospaced where useful

Body prose is capped at 70 characters. Hierarchy comes from size, weight, and space rather than extra font families.

## 4. Elevation

Koma is flat at rest. Canvas, surface tone, one-pixel separators, and occlusion define structure.

- Resting library surfaces: no shadow.
- Selected books: a one-pixel accent edge plus a two-pixel soft outline.
- Menus, command search, and dialogs: one earned shadow because they temporarily sit above the application.
- Side panels: an edge separator and directional shadow only while overlaid.
- Reader controls: opaque dark surfaces rather than decorative blur.

Corner radii are 6px, 9px, and 14px. Pills are reserved for true continuous controls such as progress tracks and switches, not ordinary buttons or containers.

Motion uses 140ms for direct feedback and 190ms for panels or state changes. Animations use opacity or transform with ease-out timing, never bounce and never layout properties. `prefers-reduced-motion` removes nonessential transitions.

## 5. Components

- **Application shell:** persistent wide sidebar, adaptive toolbar, contextual inspector, and phone bottom navigation.
- **Book collection:** virtualized grid or list, natural selection, progress, format, favorite, missing-source, and context-menu states.
- **Continue feature:** one dominant publication with a direct reading action.
- **Reader:** auto-hiding toolbar and scrubber, single, spread, continuous, webtoon, panel-focus, and presentation canvases.
- **Reader settings:** a contextual side sheet on wide screens and bottom sheet on phones, with labeled sliders, switches, direction, fit, and image controls.
- **Command search:** keyboard-first access to navigation, import, settings, and publication tools.
- **Source importer:** paste, choose a volume or full series, select a destination, confirm access, then follow download progress in one linear panel.
- **Publication workbench:** inspect, edit metadata, convert, and repair without crowding the library.
- **Feedback:** inline errors for local problems, toasts for completed actions, password dialog only when archive or PDF encryption requires it.
- **Empty states:** one clear sentence and at most two immediate actions.

Every interactive component provides a visible focus state, accessible name, keyboard behavior, disabled state, and a minimum touch target appropriate to its layout.

## 6. Do's and Don'ts

### Do

- Let comic pages own the visual center.
- Keep ordinary actions immediate and advanced tools one deliberate step away.
- Prefer spacing and tonal contrast before adding a border or container.
- Preserve familiar platform behavior for files, menus, focus, shortcuts, touch, and window state.
- Use concise, literal copy and tell the user what remains unchanged after a destructive-looking operation.
- Verify every responsive state at its real viewport, including safe areas.

### Don't

- Do not use gradient text, glass cards, decorative blobs, fake paper, novelty Japanese motifs, or anime-themed chrome.
- Do not expose every reader option at once.
- Do not use identical dashboard cards or nested cards.
- Do not use colored side stripes as decoration.
- Do not tint comic artwork or use the persimmon accent as ambient decoration.
- Do not copy layouts, assets, or source from CBZen, Koharu, Panels, Copi, or another reader.
- Do not claim a workflow is supported without a test, fixture, runtime check, or explicit limitation.

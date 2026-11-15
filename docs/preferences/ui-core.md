# UI design core

## 1. Purposeful pixels

Every pixel on the interface must serve a purpose, whether it's carrying information, guiding the user's eye or thoughts, or leaving room open needed to do those jobs effectively. If there is no value gained for the customer by keeping the pixel, it should be stripped away. In practice, the interface doesn't contain repeated labels, decorative status pills, or instructions that merely restate the controls, and always prefers icons and color over text while providing accessible names and non-color signals for the same meaning.

## 2. No scroll jacking

Scrolling has been standard across technology (computers, mobile devices, consoles, etc.) for years. Having a scrolling behavior that the user expects is only natural. This does not mean innovation is dead and we cannot create a unique scrolling experience, it means a quality standard applies that must be met, and it is very high: high enough to make it the exception. *There are too many things that can go wrong.*

## 3. No layout shift

Interacting with the interface should not move components around if that was not the nature of the interaction. The user must not be dazed when engaging with the interface; buttons, menus, and other components must stick to their geometry.

## 4. Exploit the Cursor

The cursor is our primary medium of interaction with technology (mobile and other platforms differ of course), it is how we make decisions on the interface, so it should be the main way we get information. The worst experience is being confused about a pill, hovering over it expecting more information, only to have no additional knowledge. *CSS provides over 30 standard cursor types* for different user interactions, do not limit the interaction medium to the few common ones.

## 5. Layered clarity

While our interface should be clear and helpful, there is a line between infantilizing the user by overexplaining, versus leaving the user confused and wandering throughout the interface. Catering to both novice and more experienced users through interaction layers helps balance.

## Layers of the system

- **Principles:** Why the system makes its decisions and what correctness means. Principles are nearly immutable.
- **Design method:** How an agent approaches a UI task. The method is refined through research and evaluation.
- **Preferences:** Actionable rules for particular situations, retrieved by task when they are relevant.
- **Component contracts:** Exact API, states, behavior, and implementation boundaries, versioned with the component.
- **Project identity:** Product-owned visual and content language. It adds identity without changing portable interaction law.

## Design method

1. **Understand the job.** Identify the actor, goal or decision, constraints, assumptions, and workarounds; define success before choosing a surface.
2. **Design the information.** Set first-seen hierarchy, supporting evidence, sequence, actions, and consequences before deciding layout or style.
3. **Respect learned behavior.** Preserve platform conventions for scrolling, focus, dismissal, and input. Novel behavior needs a clear improvement and complete behavior.
4. **Prefer the simplest complete solution.** Remove repetition, ornament, and unnecessary containers; never remove required context, feedback, accessibility, or capability. Use the fewest elements that fully support the task.
5. **Prove advertised behavior.** Implement and inspect actual states and interactions with realistic content and failure cases. Examples cannot label behavior they do not implement.
6. **Search before inventing.** Use the lookup sequence and compose existing pieces. Create a new abstraction only for a genuinely missing stable, shared job.
7. **Apply product identity.** After information and behavior are settled, apply project typography, color, composition, and voice. Differentiation must not rewrite portable interaction law.

## Lookup sequence

Follow these steps in order. Each reads one layer of the system, from the most stable to the most specific.

1. **Load the UI Core.** Every interface task starts with the [UI Core](./ui-core.md), which holds the principles, the method, and this sequence.
2. **Search the preference index.** Match your task to a topic in [ui-index.json](./ui-index.json) and read only that section of [ui.md](./ui.md). From a shell, `rg -i "focus" ui-index.json` finds the topic and `rg -n "^## Shared component state rules" ui.md` jumps to its section.
3. **Find the owning component.** Run `pnpm exec opifex search dropdown menu --json --limit 8` to list candidates and `pnpm exec opifex inspect dropdown-menu --json` to read its props, states, and keyboard behavior. When the CLI is unavailable, read the [catalog](./catalog.json) and a contract such as [button.json](./r/button.json).
4. **Create only what is missing.** Compose existing components first, and when nothing owns the behavior, run `pnpm exec opifex skills add opifex-author-component --agent codex` and write the contract before the code.
5. **Load project identity separately.** Type, color, composition, and voice belong to the product, so they stay out of the portable rules.

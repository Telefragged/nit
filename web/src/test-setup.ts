import { afterEach } from "vitest";

// Each test opens with an empty browser store. A page that remembers a view
// choice — the diff mode, the change graph's grouping — would otherwise
// carry the choice of one test into the next.
afterEach(() => {
  localStorage.clear();
});

// jsdom has no top-layer, so a <dialog> carries no showModal(); opening a
// modal throws without this stub.
HTMLDialogElement.prototype.showModal = function () {
  this.open = true;
};

I want to build a tool that lets expert programmers efficiently review code produced by AI coding tools.

It should be tool-agnostic and connect in a way that lets the agent register their changes, and lets them automatically resume when a user provides feedback or approves their changes.

The user (expert programmer) will interact with a webpage. You can decide the architecture, but the review page itself should:
- Have a diff-view similar to diffshub.com
- Allow users to comment on specific lines of changes, like gerrit, and approve or request changes on the commit with said comments.
    - Also, like in gerrit, comments should be in a draft state before the user chooses to submit them all.

The unit for review in this case will to begin with be a single commit like it is in gerrit.
The optimal workflow for this tool would then naturally be agents making smaller commits and presenting them to the user instead of a single branch.
The other natural fit here is that agents, when receiving feedback on their changes, amend the commit the reviews are directed at and push the rewritten branch — a Change-Id trailer keeps each commit's identity across the rewrite.

On top of this, agents might want to make multiple changes before submitting for review, so they should register their entire branch as a change, while the tool presents each commit individually.

The main page of the application should show all the "branches" (or relation chains from gerrit) and their current state (waiting for review or agent's turn). When a one of these branches is done and the agent merges or rebases it it should be removed from the page.

You can choose the architecture, frameworks and tools to use, but I have one precondition:
All of the tools must be available with a nix shell (flake devShell kinda thing) and all of your development must also go via that nix shell, as well as a nix build available to build all of it.

It also needs to be possible to automatically check the frontend so that other AI agents can review the design, however that is best done.

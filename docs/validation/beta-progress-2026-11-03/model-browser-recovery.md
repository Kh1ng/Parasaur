# Model discovery remains usable when a provider is silent

The shared model browser previously waited for `Promise.all` of every provider
read and rendered only skeletons until all finished. Those fetches had neither
a deadline nor cancellation. One provider that never responded could therefore
hide the healthy model lists and every configured default indefinitely.

The browser now publishes each provider's models as its read finishes. Default
entries remain selectable during discovery, and the pending entry explains its
state. Each read has a 15-second deadline covering the response body too. A
failure identifies the affected entry and offers Retry. Closing, retrying, or
changing the profile/node aborts outstanding HTTP reads and clears their timers;
late results cannot replace the new catalog. HTTP cancellation does not claim
to terminate a provider process on the server.

The API client forwards an optional abort signal while preserving profile,
node, account-instance, authentication, and deployment-prefix handling. Model
selection and reasoning-effort behavior use the existing picker callbacks.

## Windows verification, 2026-11-04

- Seven API client unit cases passed, including exact signal forwarding with
  profile/node/instance identity under `/parasaur`.
- All six ModelPicker component cases passed. New cases leave actual browser
  fetches unanswered: healthy models and configured defaults remain usable;
  advancing the browser clock past 15 seconds produces a recoverable failure;
  Retry loads the recovered provider; closing cancels both outstanding reads;
  reopening discovers fresh results. Existing favorites, recents, keyboard
  dismissal and default selection checks remain passing.
- The composer backend-picker end-to-end workflow passed against isolated
  mock/Vite services on ports 38775/38111. The owned test services exited.
- Shared-web typechecking and the production build passed.
- Headless Chromium screenshots of the pending desktop layout and the timeout
  layout at 390 × 844 were inspected. The scrollable list and Retry/footer stay
  within the dialog; no native desktop interaction was used.

An initial new assertion expected reasoning effort to reset on a same-backend
model switch; the existing contract correctly preserves `medium`. The assertion
was corrected without changing selection behavior.

All provider responses in these tests are fixtures. No real sign-in or billed
provider request occurred. This shared-web change is local and undeployed;
the native desktop/WSL CLI installation remains candidate `c1a55cf0`.

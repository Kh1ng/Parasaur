function navigationHasContent(expected) {
  if (window !== window.top || location.origin !== expected.origin) return false;
  const root = document.getElementById('root');
  const content = root
    ? root.children.length || root.textContent.trim()
    : document.body?.innerText.trim() || document.querySelector('form, input, button, canvas, iframe');
  return Boolean(content);
}

function watchNavigationPresentation(expected) {
  if (window !== window.top || location.origin !== expected.origin) return;
  let observer;
  let expiry;
  const report = () => {
    if (!navigationHasContent(expected) || typeof window.__TAURI_INTERNALS__?.invoke !== 'function') return false;
    observer?.disconnect();
    window.clearTimeout(expiry);
    void window.__TAURI_INTERNALS__.invoke('navigation_presented', { attempt: expected.attempt }).catch(() => {});
    return true;
  };
  if (report()) return;
  // Content can appear while the native acknowledgement grace is running.
  // Stop this observer after that grace; navigation creates a fresh document.
  observer = new MutationObserver(report);
  observer.observe(document, { subtree: true, childList: true, characterData: true });
  expiry = window.setTimeout(() => observer.disconnect(), 2000);
}
